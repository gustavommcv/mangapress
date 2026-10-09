/// The publication's identifier: a name-based UUID (RFC 4122 version 5)
/// over the book's title, authors and chapter titles, in upstream's own
/// `urn:uuid:` form.
///
/// Upstream draws a random UUID per conversion. This derives it instead, so
/// converting the same book again gives the same identifier — which is what
/// EPUB 3 asks of one: the identifier names the publication and stays put
/// across its releases, and `dcterms:modified` tells the releases apart.
///
/// Version 5 rather than a hash of our own choosing because it is specified
/// bit for bit. An earlier version formatted `std`'s `DefaultHasher` output,
/// whose algorithm the standard library reserves the right to change in any
/// release: the "stable" identifier was only stable until the next compiler
/// that changed it.
pub(super) fn synthetic_identifier(seed: &str) -> String {
    let id = uuid_v5(&MANGAPRESS_NAMESPACE, seed.as_bytes());
    let hex: String = id.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// RFC 4122's namespace for names that are URLs.
#[cfg(test)]
pub(super) const URL_NAMESPACE: [u8; 16] = [
    0x6b, 0xa7, 0xb8, 0x11, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8,
];

/// The namespace book identifiers are derived in: the version 5 UUID of
/// this project's own URL, `https://github.com/gustavommcv/mangapress`, in
/// the URL namespace — a3433aff-4bf2-51ee-a1c9-836d85be953f. Fixed forever:
/// changing it changes every book's identifier.
pub(super) const MANGAPRESS_NAMESPACE: [u8; 16] = [
    0xa3, 0x43, 0x3a, 0xff, 0x4b, 0xf2, 0x51, 0xee, 0xa1, 0xc9, 0x83, 0x6d, 0x85, 0xbe, 0x95, 0x3f,
];

/// RFC 4122 version 5: SHA-1 of the namespace followed by the name, cut to
/// 16 bytes, with the version and variant bits set.
pub(super) fn uuid_v5(namespace: &[u8; 16], name: &[u8]) -> [u8; 16] {
    let mut input = namespace.to_vec();
    input.extend_from_slice(name);
    let digest = sha1(&input);

    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id[6] = (id[6] & 0x0f) | 0x50;
    id[8] = (id[8] & 0x3f) | 0x80;
    id
}

/// SHA-1 (FIPS 180-4), for [`uuid_v5`] only — an identifier, not a security
/// boundary. Written out here rather than pulled in as a dependency for one
/// 20-byte digest per book.
pub(super) fn sha1(data: &[u8]) -> [u8; 20] {
    let mut state: [u32; 5] = [
        0x6745_2301,
        0xefcd_ab89,
        0x98ba_dcfe,
        0x1032_5476,
        0xc3d2_e1f0,
    ];

    // The message, a single 1 bit, zeros up to 8 bytes short of a 64-byte
    // block, then the message's length in bits.
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        let mut w = [0u32; 80];
        for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }

        let [mut a, mut b, mut c, mut d, mut e] = state;
        for (i, &word) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a82_7999),
                20..=39 => (b ^ c ^ d, 0x6ed9_eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let next = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = next;
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut digest = [0u8; 20];
    for (chunk, word) in digest.as_chunks_mut::<4>().0.iter_mut().zip(state) {
        *chunk = word.to_be_bytes();
    }
    digest
}
