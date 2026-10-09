//! Natural (human) ordering for filenames/paths, after what KCC does to the pages of a folder
//! (`sanitizeTree()` in `comic2ebook.py`): it puts the names of each folder in order with the
//! `natsort` library's operating-system ordering. Runs of digits compare as numbers, so
//! `page2.jpg` sorts before `page10.jpg`, and case is ignored. Used both for ordering extracted
//! archive entries ([`crate::archive::cbz`]) and for the folder reader.
//!
//! What `natsort` does to a name, and this module with it:
//!
//! - The name is split in a stem and its extensions, and the stem is compared first. `p01.png`
//!   sorts before `p01 (2).png` and `1.png` before `1.5.png`, as the names themselves do. Up to
//!   two extensions are split off, each of at most five characters, and none that starts with a
//!   digit (`1.5` is a stem, `.png` an extension).
//! - A number is a run of decimal digits of any script (`１０`, `٣`), or one character that
//!   stands for a digit (`²`, `①`). See [`digits`].
//! - Case is folded with the language's lower-casing, where `natsort` folds it fully (`ß` is
//!   `ss` to it). The text between numbers is compared by code point, which is what `natsort`
//!   gets on a system whose locale collates by code point. On any other it follows the locale,
//!   so the order of punctuation, accents and the like is the one thing that differs from KCC
//!   there (ADR 0019, ORD-5).

mod digits;

use digits::{digit, Digit};
use std::cmp::Ordering;
use std::path::Path;

/// A number of any size, as its digits without leading zeros.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Number(String);

impl Ord for Number {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .len()
            .cmp(&other.0.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl PartialOrd for Number {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Number {
    fn new(digits: &str) -> Self {
        let trimmed = digits.trim_start_matches('0');
        Self(if trimmed.is_empty() { "0" } else { trimmed }.to_owned())
    }
}

/// Text and numbers take turns, starting with text (empty when the name starts with a number),
/// so that two keys never compare a text with a number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum KeyPart {
    Text(String),
    Num(Number),
}

/// The key of one piece of a name: its text and numbers, in turn.
fn piece_key(piece: &str) -> Vec<KeyPart> {
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut run = String::new();

    let end_run = |parts: &mut Vec<KeyPart>, text: &mut String, run: &mut String| {
        if !run.is_empty() {
            parts.push(KeyPart::Text(std::mem::take(text)));
            parts.push(KeyPart::Num(Number::new(run)));
            run.clear();
        }
    };

    for c in piece.to_lowercase().chars() {
        match digit(c) {
            Some(Digit::Decimal(value)) => run.push(char::from(b'0' + value)),
            Some(Digit::Single(value)) => {
                end_run(&mut parts, &mut text, &mut run);
                parts.push(KeyPart::Text(std::mem::take(&mut text)));
                parts.push(KeyPart::Num(Number::new(&value.to_string())));
            }
            None => {
                end_run(&mut parts, &mut text, &mut run);
                text.push(c);
            }
        }
    }
    end_run(&mut parts, &mut text, &mut run);
    if !text.is_empty() {
        parts.push(KeyPart::Text(text));
    }
    parts
}

/// The extensions of a name that are split from its stem: the last two at most, each of at
/// most five characters, stopping at one that starts with a digit.
fn extensions(name: &str) -> Vec<String> {
    let all: Vec<String> = name
        .trim_start_matches('.')
        .split('.')
        .skip(1)
        .map(|extension| format!(".{extension}"))
        .collect();
    let mut kept = Vec::new();
    for (position, extension) in all.iter().rev().enumerate() {
        let starts_with_digit = extension
            .chars()
            .nth(1)
            .is_some_and(|c| matches!(digit(c), Some(Digit::Decimal(_))));
        if starts_with_digit || position > 1 || extension.chars().count() > 5 {
            break;
        }
        kept.push(extension.clone());
    }
    kept.reverse();
    kept
}

/// The key of a name (one file or folder name, not a path): its stem, then each extension.
fn natural_key(name: &str) -> Vec<Vec<KeyPart>> {
    let extensions = extensions(name);
    let joined = extensions.concat();
    let stem = if joined.is_empty() {
        name.to_owned()
    } else {
        name.replace(&joined, "")
    };
    std::iter::once(stem)
        .chain(extensions)
        .filter(|piece| !piece.is_empty())
        .map(|piece| piece_key(&piece))
        .collect()
}

/// Compare two plain strings (e.g. file or directory names) in natural order.
pub fn compare(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b))
}

/// Compare two paths in natural order, component by component — so a
/// number inside one path segment is never compared against a number in a
/// different segment as if the whole path were one flat string.
///
/// Where one path has a file and the other a folder at the same level, the file comes
/// first: a folder's own pages come before the folders inside it, as upstream's walk puts
/// them (a `cover.png` beside the chapter folders is the book's first page, whatever the
/// chapters are called).
pub fn compare_paths(a: &Path, b: &Path) -> Ordering {
    let parts = |path: &Path| -> Vec<String> {
        path.components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect()
    };
    let (a, b) = (parts(a), parts(b));
    for (level, (left, right)) in a.iter().zip(&b).enumerate() {
        let (left_key, right_key) = (natural_key(left), natural_key(right));
        if left_key == right_key {
            continue;
        }
        let left_is_file = level + 1 == a.len();
        let right_is_file = level + 1 == b.len();
        return match (left_is_file, right_is_file) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => left_key.cmp(&right_key),
        };
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
#[path = "natural_sort/tests.rs"]
mod tests;
