use super::*;

fn picture(width: u32, height: u32) -> image::RgbImage {
    image::RgbImage::from_fn(width, height, |x, y| {
        image::Rgb([
            (x * 3 + y) as u8,
            (y * 2 + x / 2) as u8,
            ((x ^ y) * 5) as u8,
        ])
    })
}

#[test]
fn a_gray_picture_round_trips_close_to_itself() {
    let gray = image::DynamicImage::ImageRgb8(picture(64, 48)).to_luma8();
    let bytes = encode(gray.as_raw(), 64, 48, Samples::Gray, 90).unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap().to_luma8();
    assert_eq!(decoded.dimensions(), (64, 48));
    let total: u32 = gray
        .pixels()
        .zip(decoded.pixels())
        .map(|(a, b)| u32::from(a.0[0].abs_diff(b.0[0])))
        .sum();
    assert!(
        total / (64 * 48) < 4,
        "mean difference {}",
        total / (64 * 48)
    );
}

#[test]
fn a_color_picture_keeps_its_chroma_at_half_size_in_both_directions() {
    let source = picture(64, 48);
    let bytes = encode(source.as_raw(), 64, 48, Samples::Rgb, 85).unwrap();
    // The frame header lists each component's sampling factors; luma is 2x2, chroma 1x1.
    let sof = bytes
        .windows(2)
        .position(|marker| marker == [0xFF, 0xC0])
        .expect("a baseline frame");
    let components = &bytes[sof + 10..sof + 19];
    assert_eq!([components[0], components[3], components[6]], [1, 2, 3]);
    let sos = bytes
        .windows(2)
        .position(|marker| marker == [0xFF, 0xDA])
        .unwrap();
    assert_eq!([bytes[sos + 5], bytes[sos + 7], bytes[sos + 9]], [1, 2, 3]);
    assert_eq!(components[1], 0x22, "luma sampling");
    assert_eq!(components[4], 0x11, "chroma sampling");
    assert_eq!(components[7], 0x11, "chroma sampling");
    let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
    assert_eq!(decoded.dimensions(), (64, 48));
}

/// The ids of the quantization tables a file defines.
fn table_ids(bytes: &[u8]) -> Vec<u8> {
    let mut ids = Vec::new();
    let mut at = 2;
    while bytes[at] == 0xFF && bytes[at + 1] != 0xDA {
        let length = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        if bytes[at + 1] == 0xDB {
            ids.push(bytes[at + 4] & 0x0F);
        }
        at += 2 + length;
    }
    ids
}

#[test]
fn a_gray_picture_has_one_component_and_one_table() {
    let bytes = encode(&[128; 16 * 16], 16, 16, Samples::Gray, 85).unwrap();
    let sof = bytes
        .windows(2)
        .position(|marker| marker == [0xFF, 0xC0])
        .unwrap();
    assert_eq!(bytes[sof + 9], 1, "number of components");
    assert_eq!(table_ids(&bytes), [0]);
    assert!(image::load_from_memory(&bytes).is_ok());
}

#[test]
fn a_color_picture_keeps_both_tables() {
    let source = picture(32, 32);
    let bytes = encode(source.as_raw(), 32, 32, Samples::Rgb, 85).unwrap();
    assert_eq!(table_ids(&bytes), [0, 1]);
}

#[test]
fn the_huffman_tables_come_from_the_picture() {
    // A flat picture needs few symbols: with tables built from it the file is smaller
    // than with the standard ones, which carry every symbol.
    let flat = vec![200u8; 256 * 256];
    let optimized = encode(&flat, 256, 256, Samples::Gray, 85).unwrap();
    let mut standard = Vec::new();
    let encoder = Encoder::new(&mut standard, 85);
    encoder.encode(&flat, 256, 256, ColorType::Luma).unwrap();
    assert!(optimized.len() < standard.len());
}

#[test]
fn a_side_longer_than_a_jpeg_can_be_is_an_error() {
    assert!(matches!(
        encode(&[0; 70_000], 70_000, 1, Samples::Gray, 85),
        Err(Error::Encode(_))
    ));
}

#[test]
fn the_quality_scales_the_standard_tables() {
    let source = picture(64, 48);
    let low = encode(source.as_raw(), 64, 48, Samples::Rgb, 20).unwrap();
    let high = encode(source.as_raw(), 64, 48, Samples::Rgb, 95).unwrap();
    assert!(low.len() < high.len());
}

#[test]
fn colors_come_back_the_right_ones_through_the_image_crates_decoder() {
    let source = image::RgbImage::from_fn(48, 48, |x, y| match (x / 16, y / 16) {
        (0, _) => image::Rgb([220, 30, 30]),
        (1, _) => image::Rgb([30, 200, 40]),
        _ => image::Rgb([40, 40, 220]),
    });
    let bytes = encode(source.as_raw(), 48, 48, Samples::Rgb, 90).unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap().to_rgb8();
    for (x, wanted) in [
        (8, [220i32, 30, 30]),
        (24, [30, 200, 40]),
        (40, [40, 40, 220]),
    ] {
        let got = decoded.get_pixel(x, 24).0;
        for channel in 0..3 {
            assert!(
                (i32::from(got[channel]) - wanted[channel]).abs() < 12,
                "{got:?} against {wanted:?}"
            );
        }
    }
}

#[test]
fn the_conversion_to_y_cb_cr_is_jfifs_in_libjpegs_fixed_point() {
    // White, black, red, green.
    let planes = Planes::new(&[255, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0], 4, 1);
    assert_eq!(planes.luma, [255, 0, 76, 150]);
    // Cb is 128, 128, 85, 44 and Cr 128, 128, 255, 21; reduced in pairs (a picture one row
    // high repeats its row): (2*128 + 2*128 + 1) / 4, and (2*85 + 2*44 + 2) / 4 for Cb.
    assert_eq!(planes.blue, [128, 128, 65, 65]);
    assert_eq!(planes.red, [128, 128, 138, 138]);
}

#[test]
fn chroma_is_reduced_by_four_with_a_bias_that_alternates() {
    // Two squares of 0 and of 1 in a row: the sums are 0 and 4.
    let plane = [0, 0, 1, 1, 0, 0, 1, 1];
    assert_eq!(reduce_and_repeat(&plane, 4, 2), plane);
    // A square of sum 7 rounds down with bias 1 and up with bias 2: (7+1)>>2 and (7+2)>>2.
    let plane = [2, 2, 2, 1, 2, 2, 2, 1];
    assert_eq!(
        reduce_and_repeat(&plane, 4, 2),
        [2, 2, 1 + 1, 1 + 1, 2, 2, 2, 2]
    );
}

#[test]
fn an_edge_in_the_middle_of_a_square_repeats_its_last_pixel() {
    let plane = [4, 8, 12];
    // First square: 4+8+4+8 = 24, bias 1 -> 6; second: 12 four times = 48, bias 2 -> 12.
    assert_eq!(reduce_and_repeat(&plane, 3, 1), [6, 6, 12]);
}
