//! Natural (human) ordering for filenames/paths — matches KCC's own
//! `walkSort()` (`shared.py`) and the `natsort` library's `os_sort_keygen()`
//! used in `sanitizeTree()`: runs of digits compare numerically rather than
//! character-by-character, so `page2.jpg` sorts before `page10.jpg`. Used
//! both for ordering extracted archive entries ([`crate::archive::cbz`])
//! and, later, chapter/page ordering when building output ebooks.

use std::cmp::Ordering;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum KeyPart {
    Num(u64),
    Text(String),
}

fn natural_key(segment: &str) -> Vec<KeyPart> {
    let mut parts = Vec::new();
    let mut num = String::new();
    let mut text = String::new();

    // Case is ignored, as upstream ignores it (`walkSort()` lowercases every
    // name before splitting it): "chapter 2" sorts between "Chapter 1" and
    // "Chapter 10", not after every capitalized name. An earlier version
    // compared the text runs as written, which put all uppercase-initial
    // names first.
    for c in segment.to_lowercase().chars() {
        if c.is_ascii_digit() {
            if !text.is_empty() {
                parts.push(KeyPart::Text(std::mem::take(&mut text)));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                parts.push(KeyPart::Num(num.parse().unwrap_or(u64::MAX)));
                num.clear();
            }
            text.push(c);
        }
    }
    if !num.is_empty() {
        parts.push(KeyPart::Num(num.parse().unwrap_or(u64::MAX)));
    }
    if !text.is_empty() {
        parts.push(KeyPart::Text(text));
    }
    parts
}

fn path_key(path: &Path) -> Vec<Vec<KeyPart>> {
    path.components()
        .map(|c| natural_key(&c.as_os_str().to_string_lossy()))
        .collect()
}

/// Compare two plain strings (e.g. directory basenames) in natural order.
pub fn compare(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b))
}

/// Compare two paths in natural order, component by component — so a
/// number inside one path segment is never compared against a number in a
/// different segment as if the whole path were one flat string.
pub fn compare_paths(a: &Path, b: &Path) -> Ordering {
    path_key(a).cmp(&path_key(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn numeric_runs_compare_numerically_not_lexicographically() {
        assert_eq!(compare("page2.jpg", "page10.jpg"), Ordering::Less);
    }

    #[test]
    fn equal_strings_compare_equal() {
        assert_eq!(compare("page002.jpg", "page002.jpg"), Ordering::Equal);
    }

    #[test]
    fn case_does_not_affect_the_order() {
        // Upstream's `walkSort()` on these names gives exactly this order.
        let mut names = vec!["Chapter 10", "chapter 2", "Chapter 1", "b.jpg", "A.jpg"];
        names.sort_by(|a, b| compare(a, b));
        assert_eq!(
            names,
            ["A.jpg", "b.jpg", "Chapter 1", "chapter 2", "Chapter 10"]
        );
    }

    #[test]
    fn purely_textual_strings_fall_back_to_lexicographic() {
        assert_eq!(compare("apple.jpg", "banana.jpg"), Ordering::Less);
    }

    #[test]
    fn chapter_paths_sort_by_directory_first_then_by_file() {
        let a = PathBuf::from("c001 - Title One/page002.jpg");
        let b = PathBuf::from("c002 - Title Two/page001.jpg");
        assert_eq!(compare_paths(&a, &b), Ordering::Less);
    }

    #[test]
    fn double_digit_chapter_sorts_after_single_digit() {
        let a = PathBuf::from("c2 - Two/page001.jpg");
        let b = PathBuf::from("c10 - Ten/page001.jpg");
        assert_eq!(compare_paths(&a, &b), Ordering::Less);
    }
}
