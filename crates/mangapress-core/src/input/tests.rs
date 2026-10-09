use super::*;

#[test]
fn only_supported_page_codecs_are_enabled() {
    let supported = [
        image::ImageFormat::Jpeg,
        image::ImageFormat::Png,
        image::ImageFormat::Gif,
        image::ImageFormat::Bmp,
        image::ImageFormat::WebP,
    ];
    // Cargo unifies features across dependencies, including dev-dependencies.
    // Catch a dependency accidentally enabling the default codecs again.
    for format in image::ImageFormat::all() {
        assert_eq!(
            format.reading_enabled(),
            supported.contains(&format),
            "{format:?}"
        );
        assert_eq!(
            format.writing_enabled(),
            supported.contains(&format),
            "{format:?}"
        );
    }
}

#[test]
fn content_sniffing_does_not_enable_an_unsupported_decoder() {
    let pnm = b"P6\n1 1\n255\n\x01\x02\x03";
    assert_eq!(image::guess_format(pnm).unwrap(), image::ImageFormat::Pnm);
    for error in [
        image_dimensions(pnm).unwrap_err(),
        decode_image(pnm).unwrap_err(),
    ] {
        assert!(matches!(
            error,
            Error::Image(image::ImageError::Unsupported(_))
        ));
    }
}

#[test]
fn declared_size_is_rejected_before_reading() {
    struct MustNotRead;
    impl Read for MustNotRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            panic!("an oversized declaration must not be read")
        }
    }
    let error = read_bounded(MustNotRead, Path::new("page.png"), 9, 8).unwrap_err();
    assert!(matches!(error, Error::InputTooLarge { limit: 8, .. }));
}

#[test]
fn actual_bytes_are_bounded_even_when_the_declared_size_is_small() {
    let mut bytes = Cursor::new([0; 32]);
    let error = read_bounded(&mut bytes, Path::new("page.png"), 1, 8).unwrap_err();
    assert!(matches!(error, Error::InputTooLarge { limit: 8, .. }));
    assert_eq!(bytes.position(), 9, "do not consume the remaining input");
}

#[test]
fn the_exact_byte_limit_and_short_reads_are_allowed() {
    assert_eq!(
        read_bounded(Cursor::new([3; 8]), Path::new("page.png"), 8, 8).unwrap(),
        [3; 8]
    );
    assert_eq!(
        read_bounded(Cursor::new([3; 4]), Path::new("page.png"), 8, 8).unwrap(),
        [3; 4]
    );
}

#[test]
fn image_area_matches_the_reference_limit_without_integer_overflow() {
    assert!(check_dimensions(MAX_IMAGE_PIXELS as u32, 1, MAX_IMAGE_PIXELS).is_ok());
    for (width, height) in [(MAX_IMAGE_PIXELS as u32 + 1, 1), (u32::MAX, u32::MAX)] {
        assert!(matches!(
            check_dimensions(width, height, MAX_IMAGE_PIXELS),
            Err(Error::ImageTooLarge { .. })
        ));
    }
    assert!(
        check_dimensions(1, 100_000, MAX_IMAGE_PIXELS).is_ok(),
        "long strips are not a square dimension limit"
    );
}

#[test]
fn an_oversized_image_header_is_refused_before_decoding_pixels() {
    let bytes = crate::test_support::oversized_bmp();
    assert!(bytes.len() < 1024);
    for error in [
        image_dimensions(&bytes).unwrap_err(),
        decode_image(&bytes).unwrap_err(),
    ] {
        assert!(matches!(
            error,
            Error::ImageTooLarge {
                width: 50_000,
                height: 50_000,
                ..
            }
        ));
    }
}

#[test]
fn kccs_larger_image_limit_is_not_replaced_by_pillows_default() {
    let bytes = crate::test_support::bmp_with_dimensions(20_000, 20_000);
    assert_eq!(image_dimensions(&bytes).unwrap(), (20_000, 20_000));
    assert!(check_webtoon_dimensions(MAX_WEBTOON_PIXELS as u32, 1).is_ok());
    assert!(check_webtoon_dimensions(MAX_WEBTOON_PIXELS as u32 + 1, 1).is_err());
}

#[test]
fn the_existing_decoder_allocation_limit_is_still_enforced() {
    let bytes = crate::test_support::bmp_with_dimensions(20_000, 20_000);
    assert!(matches!(
        decode_image(&bytes),
        Err(Error::Image(image::ImageError::Limits(_)))
    ));
}

#[test]
fn ordinary_images_keep_the_image_crates_pixels_and_color_type() {
    for image in [
        DynamicImage::ImageLuma8(image::GrayImage::from_pixel(3, 7, image::Luma([89]))),
        DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            3,
            7,
            image::Rgba([2, 5, 8, 11]),
        )),
    ] {
        for format in [image::ImageFormat::Png, image::ImageFormat::Bmp] {
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            let expected = image::load_from_memory(bytes.get_ref()).unwrap();
            let actual = decode_image(bytes.get_ref()).unwrap();
            assert_eq!(actual.color(), expected.color());
            assert_eq!(actual.as_bytes(), expected.as_bytes());
        }
    }
}

#[test]
fn decoder_and_io_errors_are_not_hidden() {
    assert!(matches!(
        decode_image(b"not an image"),
        Err(Error::Image(_))
    ));
    struct BrokenReader;
    impl Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "test failure",
            ))
        }
    }
    assert!(matches!(
        read_bounded(BrokenReader, Path::new("page.png"), 1, 8),
        Err(Error::Io(_))
    ));
}
// ---- Pages whose file ends early ----

fn encoded(image: &DynamicImage, format: image::ImageFormat) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, format).unwrap();
    bytes.into_inner()
}

/// Rows that differ from each other and do not compress away: row `y` is `y + 1` in every
/// sample, with noise that makes a cut in the data land in the middle of the image.
fn noisy_rows(width: u32, height: u32) -> image::RgbImage {
    image::RgbImage::from_fn(width, height, |x, y| {
        let noise = (x * 7 + y * 13 + x * y) as u8;
        image::Rgb([noise, noise.wrapping_mul(3), (y + 1) as u8])
    })
}

fn cut(bytes: &[u8], tenths: usize) -> &[u8] {
    &bytes[..bytes.len() * tenths / 10]
}

#[test]
fn a_whole_page_is_not_truncated() {
    let bytes = encoded(
        &DynamicImage::ImageRgb8(noisy_rows(40, 60)),
        image::ImageFormat::Png,
    );
    assert!(!decode_page(&bytes).unwrap().truncated);
}

#[test]
fn a_png_cut_short_keeps_the_rows_it_has_and_the_rest_is_black() {
    let source = noisy_rows(60, 120);
    let bytes = encoded(
        &DynamicImage::ImageRgb8(source.clone()),
        image::ImageFormat::Png,
    );
    let page = decode_page(cut(&bytes, 6)).unwrap();
    assert!(page.truncated);
    let page = page.image.to_rgb8();
    assert_eq!(page.dimensions(), source.dimensions());

    let kept = (0..source.height())
        .take_while(|&y| {
            (0..source.width()).all(|x| page.get_pixel(x, y) == source.get_pixel(x, y))
        })
        .count() as u32;
    assert!(kept > 10 && kept < source.height(), "{kept} rows were kept");
    for y in kept..source.height() {
        assert!(
            (0..source.width()).all(|x| page.get_pixel(x, y).0 == [0, 0, 0]),
            "row {y} should be black"
        );
    }
}

#[test]
fn a_palette_png_cut_short_starts_from_its_first_color() {
    let (width, height) = (60u32, 120u32);
    let indices: Vec<u8> = (0..width * height)
        .map(|n| 1 + ((u64::from(n) * 2_654_435_761) >> 7) as u8 % 7)
        .collect();
    let palette = [
        200, 30, 30, 10, 20, 30, 40, 50, 60, 70, 80, 90, 1, 2, 3, 4, 5, 6, 7, 8, 9, 9, 8, 7,
    ];
    for (transparency, expected) in [
        (None, [200u8, 30, 30, 255]),
        (Some(vec![0u8]), [200, 30, 30, 0]),
        (Some(vec![255u8, 77]), [200, 30, 30, 255]),
    ] {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_palette(palette.to_vec());
        if let Some(transparency) = &transparency {
            encoder.set_trns(transparency.clone());
        }
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&indices).unwrap();
        writer.finish().unwrap();

        let page = decode_page(cut(&bytes, 6)).unwrap();
        assert!(page.truncated);
        let page = page.image.to_rgba8();
        let last = page.get_pixel(width - 1, height - 1).0;
        if transparency.as_deref() == Some(&[0u8][..]) {
            assert_eq!(last[3], 0, "{transparency:?}");
        } else {
            assert_eq!(last, expected, "{transparency:?}");
        }
    }
}

#[test]
fn a_gray_png_with_a_transparent_black_starts_transparent_and_any_other_opaque() {
    let (width, height) = (60u32, 120u32);
    let pixels: Vec<u8> = (0..width * height)
        .map(|n| 1 + ((u64::from(n) * 2_654_435_761) >> 7) as u8 % 200)
        .collect();
    for (transparent_gray, alpha) in [(0u8, 0u8), (255, 255)] {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_trns(vec![0, transparent_gray]);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
        writer.finish().unwrap();

        let page = decode_page(cut(&bytes, 6)).unwrap();
        assert!(page.truncated);
        let last = page
            .image
            .to_luma_alpha8()
            .get_pixel(width - 1, height - 1)
            .0;
        assert_eq!(last, [0, alpha]);
    }
}

#[test]
fn a_16_bit_png_cut_short_keeps_its_samples_in_the_machines_order() {
    let source = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(60, 120, |x, y| {
        image::Rgb([
            (x * 977 + y * 31) as u16,
            (y * 541 + x * x) as u16,
            (1000 + y) as u16,
        ])
    });
    let bytes = encoded(
        &DynamicImage::ImageRgb16(source.clone()),
        image::ImageFormat::Png,
    );
    let page = decode_page(cut(&bytes, 6)).unwrap();
    assert!(page.truncated);
    let page = page.image.to_rgb16();
    assert_eq!(page.get_pixel(5, 0), source.get_pixel(5, 0));
    assert_eq!(page.get_pixel(59, 3), source.get_pixel(59, 3));
    assert_eq!(page.get_pixel(0, 119).0, [0, 0, 0]);
}

/// An interlaced (Adam7) 8-bit gray PNG written by hand, its data in stored deflate blocks:
/// the encoder in use does not write interlaced images.
fn interlaced_gray_png(width: u32, height: u32, sample: impl Fn(u32, u32) -> u8) -> Vec<u8> {
    let mut data = Vec::new();
    for (x0, y0, dx, dy) in [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ] {
        for y in (y0..height).step_by(dy) {
            if x0 < width {
                data.push(0);
                data.extend((x0..width).step_by(dx).map(|x| sample(x, y)));
            }
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &byte in &data {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    let mut zlib = vec![0x78, 0x01];
    let blocks: Vec<&[u8]> = data.chunks(60000).collect();
    for (n, block) in blocks.iter().enumerate() {
        zlib.push(u8::from(n + 1 == blocks.len()));
        zlib.extend((block.len() as u16).to_le_bytes());
        zlib.extend((!(block.len() as u16)).to_le_bytes());
        zlib.extend(*block);
    }
    zlib.extend(((b << 16) | a).to_be_bytes());

    let crc32 = |bytes: &[u8]| {
        let mut crc = u32::MAX;
        for &byte in bytes {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xEDB8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        !crc
    };
    let chunk = |kind: &[u8; 4], body: &[u8]| {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        let mut checked = kind.to_vec();
        checked.extend(body);
        out.extend(&checked);
        out.extend(crc32(&checked).to_be_bytes());
        out
    };
    let mut header = width.to_be_bytes().to_vec();
    header.extend(height.to_be_bytes());
    header.extend([8, 0, 0, 0, 1]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    png.extend(chunk(b"IHDR", &header));
    png.extend(chunk(b"IDAT", &zlib));
    png.extend(chunk(b"IEND", b""));
    png
}

#[test]
fn an_interlaced_png_cut_short_leaves_the_rows_of_the_passes_it_did_not_reach() {
    let sample = |x: u32, y: u32| 1 + ((x * 5 + y * 3) % 250) as u8;
    let bytes = interlaced_gray_png(24, 24, sample);
    assert!(!decode_page(&bytes).unwrap().truncated);

    let page = decode_page(cut(&bytes, 5)).unwrap();
    assert!(page.truncated);
    let page = page.image.to_luma8();
    assert_eq!(
        page.get_pixel(0, 0).0[0],
        sample(0, 0),
        "the first pass is read first"
    );
    assert_eq!(
        page.get_pixel(23, 23).0[0],
        0,
        "the last pass was not reached"
    );
    let blank = page.pixels().filter(|p| p.0[0] == 0).count();
    assert!(blank > 0 && blank < 24 * 24, "{blank} pixels are blank");
}

#[test]
fn what_a_gif_has_before_its_data_is_read() {
    // Header, a screen with a 4-color table, then the first image.
    let mut gif = b"GIF89a".to_vec();
    gif.extend([4, 0, 4, 0, 0x81, 0, 0]);
    gif.extend([10, 11, 12, 20, 21, 22, 30, 31, 32, 40, 41, 42]);
    let image = [0x2C, 0, 0, 0, 0, 4, 0, 4, 0, 0];
    let control = |transparent: u8| [0x21, 0xF9, 4, 1, 0, 0, transparent, 0];

    let mut plain = gif.clone();
    plain.extend(image);
    assert_eq!(gif_unread_pixel(&plain), Some(vec![10, 11, 12, 255]));

    let mut transparent_zero = gif.clone();
    transparent_zero.extend(control(0));
    transparent_zero.extend(image);
    assert_eq!(
        gif_unread_pixel(&transparent_zero),
        Some(vec![10, 11, 12, 0])
    );

    let mut transparent_two = gif.clone();
    transparent_two.extend(control(2));
    transparent_two.extend(image);
    assert_eq!(
        gif_unread_pixel(&transparent_two),
        Some(vec![30, 31, 32, 0])
    );

    // A table of the image's own wins over the screen's.
    let mut local = gif.clone();
    local.extend([0x2C, 0, 0, 0, 0, 4, 0, 4, 0, 0x80]);
    local.extend([200, 201, 202, 1, 1, 1]);
    assert_eq!(gif_unread_pixel(&local), Some(vec![200, 201, 202, 255]));

    assert_eq!(gif_unread_pixel(&gif[..12]), None);
}

#[test]
fn a_webp_or_a_bmp_cut_short_is_still_refused() {
    let source = DynamicImage::ImageRgb8(noisy_rows(60, 120));
    for format in [image::ImageFormat::WebP, image::ImageFormat::Bmp] {
        let bytes = encoded(&source, format);
        assert!(
            matches!(decode_page(cut(&bytes, 6)), Err(Error::Image(_))),
            "{format:?}"
        );
    }
}

#[test]
fn a_png_with_a_header_and_no_data_is_a_blank_page() {
    let bytes = encoded(
        &DynamicImage::ImageRgb8(noisy_rows(20, 30)),
        image::ImageFormat::Png,
    );
    // Signature, IHDR (25 bytes) and the start of IDAT.
    let page = decode_page(&bytes[..8 + 25 + 10]).unwrap();
    assert!(page.truncated);
    assert!(page.image.to_rgb8().pixels().all(|p| p.0 == [0, 0, 0]));
}

#[test]
fn a_header_that_is_not_complete_is_still_an_error() {
    let bytes = encoded(
        &DynamicImage::ImageRgb8(noisy_rows(20, 30)),
        image::ImageFormat::Png,
    );
    assert!(decode_page(&bytes[..20]).is_err());
}
