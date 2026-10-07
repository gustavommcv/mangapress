// Tiny malformed inputs shared by the core and real-binary regression tests.
// Neither fixture contains the enormous payload its header claims.

pub fn oversized_bmp() -> Vec<u8> {
    bmp_with_dimensions(50_000, 50_000)
}

pub fn bmp_with_dimensions(width: i32, height: i32) -> Vec<u8> {
    let image = image::DynamicImage::ImageRgb8(image::RgbImage::new(1, 1));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Bmp).unwrap();
    let mut bytes = bytes.into_inner();
    bytes[18..22].copy_from_slice(&width.to_le_bytes());
    bytes[22..26].copy_from_slice(&height.to_le_bytes());
    bytes
}

pub fn zip_with_declared_size(
    entries: &[(&str, &[u8])],
    target_index: usize,
    declared_size: u64,
) -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    let mut bytes = writer.finish().unwrap().into_inner();
    let end = bytes.len() - 22; // no archive comment
    assert_eq!(&bytes[end..end + 4], b"PK\x05\x06");
    let directory_size = u32::from_le_bytes(bytes[end + 12..end + 16].try_into().unwrap());
    let mut central = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
    for _ in 0..target_index {
        let length = |offset| {
            u16::from_le_bytes(
                bytes[central + offset..central + offset + 2]
                    .try_into()
                    .unwrap(),
            ) as usize
        };
        central += 46 + length(28) + length(30) + length(32);
    }
    assert_eq!(&bytes[central..central + 4], b"PK\x01\x02");
    let name_length =
        u16::from_le_bytes(bytes[central + 28..central + 30].try_into().unwrap()) as usize;
    let extra_length = u16::from_le_bytes(bytes[central + 30..central + 32].try_into().unwrap());
    // ZIP64's extra field supplies the uncompressed size marked as 0xffffffff
    // in the central header. The compressed bytes remain the tiny real payload.
    bytes[central + 24..central + 28].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[central + 30..central + 32].copy_from_slice(&(extra_length + 12).to_le_bytes());
    let mut extra = vec![1, 0, 8, 0]; // ZIP64 field ID and eight-byte size
    extra.extend_from_slice(&declared_size.to_le_bytes());
    bytes.splice(
        central + 46 + name_length..central + 46 + name_length,
        extra,
    );
    let end = end + 12;
    bytes[end + 12..end + 16].copy_from_slice(&(directory_size + 12).to_le_bytes());
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    assert_eq!(
        archive.by_index_raw(target_index).unwrap().size(),
        declared_size
    );
    bytes
}
