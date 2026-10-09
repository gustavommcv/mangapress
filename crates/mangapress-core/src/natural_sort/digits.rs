//! Which characters are digits when names are put in order, and what each is worth.
//!
//! KCC orders names with the `natsort` library, which takes as a number a run of decimal
//! digits of any script (`１０`, `٣`) and, one at a time, the other characters that stand for
//! a digit (`²`, `①`). Rust's standard library only knows the ASCII ones, so the tables are
//! here. They are Unicode 16.0, the same version `natsort` 8.4.0 reads on Python 3.14, and are
//! printed by `python tools/parity/natsort_vectors.py --digits`.

/// The first character of each block of ten decimal digits (Unicode category Nd): a digit of
/// one of these blocks is worth its distance from the block's first character.
const DECIMAL_ZEROS: [u32; 76] = [
    0x30, 0x660, 0x6f0, 0x7c0, 0x966, 0x9e6, 0xa66, 0xae6, 0xb66, 0xbe6, 0xc66, 0xce6, 0xd66,
    0xde6, 0xe50, 0xed0, 0xf20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80, 0x1a90,
    0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0, 0xff10,
    0x104a0, 0x10d30, 0x10d40, 0x11066, 0x110f0, 0x11136, 0x111d0, 0x112f0, 0x11450, 0x114d0,
    0x11650, 0x116c0, 0x116d0, 0x116da, 0x11730, 0x118e0, 0x11950, 0x11bf0, 0x11c50, 0x11d50,
    0x11da0, 0x11f50, 0x16130, 0x16a60, 0x16ac0, 0x16b50, 0x16d70, 0x1ccf0, 0x1d7ce, 0x1d7d8,
    0x1d7e2, 0x1d7ec, 0x1d7f6, 0x1e140, 0x1e2f0, 0x1e4f0, 0x1e5f1, 0x1e950, 0x1fbf0,
];

/// Characters that stand for a digit without being decimal digits (superscripts, circled and
/// parenthesized digits, and the like), as `(first, last, value of first)`: each next character
/// is worth one more.
const OTHER_DIGITS: [(u32, u32, u8); 21] = [
    (0xb2, 0xb3, 2),
    (0xb9, 0xb9, 1),
    (0x1369, 0x1371, 1),
    (0x19da, 0x19da, 1),
    (0x2070, 0x2070, 0),
    (0x2074, 0x2079, 4),
    (0x2080, 0x2089, 0),
    (0x2460, 0x2468, 1),
    (0x2474, 0x247c, 1),
    (0x2488, 0x2490, 1),
    (0x24ea, 0x24ea, 0),
    (0x24f5, 0x24fd, 1),
    (0x24ff, 0x24ff, 0),
    (0x2776, 0x277e, 1),
    (0x2780, 0x2788, 1),
    (0x278a, 0x2792, 1),
    (0x10a40, 0x10a43, 1),
    (0x10e60, 0x10e68, 1),
    (0x11052, 0x1105a, 1),
    (0x1f100, 0x1f100, 0),
    (0x1f101, 0x1f10a, 0),
];

/// What a character is, for putting names in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Digit {
    /// A decimal digit: it joins the digits next to it into one number.
    Decimal(u8),
    /// A character that stands for a digit: it is a number by itself.
    Single(u8),
}

pub(super) fn digit(c: char) -> Option<Digit> {
    let c = u32::from(c);
    if c < 0x80 {
        return (0x30..=0x39)
            .contains(&c)
            .then(|| Digit::Decimal((c - 0x30) as u8));
    }
    if let Some(zero) = DECIMAL_ZEROS.iter().rev().find(|&&zero| zero <= c) {
        if c - zero < 10 {
            return Some(Digit::Decimal((c - zero) as u8));
        }
    }
    OTHER_DIGITS
        .iter()
        .find(|&&(first, last, _)| (first..=last).contains(&c))
        .map(|&(first, _, value)| Digit::Single(value + (c - first) as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_and_full_width_digits_are_decimal() {
        assert_eq!(digit('0'), Some(Digit::Decimal(0)));
        assert_eq!(digit('9'), Some(Digit::Decimal(9)));
        assert_eq!(digit('１'), Some(Digit::Decimal(1)));
        assert_eq!(digit('９'), Some(Digit::Decimal(9)));
        assert_eq!(digit('٣'), Some(Digit::Decimal(3)));
        assert_eq!(digit('𝟕'), Some(Digit::Decimal(7)));
    }

    #[test]
    fn superscripts_and_circled_digits_stand_alone() {
        assert_eq!(digit('²'), Some(Digit::Single(2)));
        assert_eq!(digit('¹'), Some(Digit::Single(1)));
        assert_eq!(digit('⁹'), Some(Digit::Single(9)));
        assert_eq!(digit('₀'), Some(Digit::Single(0)));
        assert_eq!(digit('①'), Some(Digit::Single(1)));
        assert_eq!(digit('⑨'), Some(Digit::Single(9)));
        assert_eq!(digit('⓪'), Some(Digit::Single(0)));
    }

    #[test]
    fn letters_punctuation_and_numerals_that_are_not_digits_are_not() {
        for c in ['a', 'Z', ' ', '.', '-', '話', 'é', 'Ⅳ', '½', '一', '〇'] {
            assert_eq!(digit(c), None, "{c}");
        }
    }

    #[test]
    fn every_block_of_decimal_digits_is_ten_in_a_row() {
        for window in DECIMAL_ZEROS.windows(2) {
            assert!(
                window[0] + 10 <= window[1],
                "blocks overlap or are out of order"
            );
        }
        for &zero in &DECIMAL_ZEROS {
            let last = char::from_u32(zero + 9).unwrap();
            assert_eq!(digit(last), Some(Digit::Decimal(9)));
        }
    }
}
