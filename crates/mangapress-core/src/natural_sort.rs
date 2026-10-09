//! Natural (human) ordering for filenames/paths, after KCC's `walkSort()` (`shared.py`): runs
//! of digits compare numerically rather than character-by-character, so `page2.jpg` sorts
//! before `page10.jpg`, and case is ignored. Used both for ordering extracted archive entries
//! ([`crate::archive::cbz`]) and for the folder reader.
//!
//! KCC also puts the files of each folder in order with the `natsort` library's
//! operating-system ordering (`sanitizeTree()`). That is not the same ordering: it compares
//! a name without its extension first, and it varies with the platform and the locale. This
//! module reproduces `walkSort()` only (ADR 0019).

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

/// Compare two plain strings (e.g. directory basenames) in natural order.
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

    #[test]
    fn a_folders_own_pages_come_before_the_folders_inside_it() {
        let mut paths = vec![
            "Chapter 2/001.png",
            "zz-credits.png",
            "Chapter 1/002.png",
            "cover.png",
            "Chapter 1/001.png",
        ];
        paths.sort_by(|a, b| compare_paths(Path::new(a), Path::new(b)));
        assert_eq!(
            paths,
            [
                "cover.png",
                "zz-credits.png",
                "Chapter 1/001.png",
                "Chapter 1/002.png",
                "Chapter 2/001.png"
            ]
        );
    }

    #[test]
    fn the_same_holds_one_level_down() {
        let mut paths = vec![
            "Vol 1/Ch 2/1.png",
            "Vol 1/zz.png",
            "Vol 1/Ch 1/1.png",
            "Vol 1/intro.png",
        ];
        paths.sort_by(|a, b| compare_paths(Path::new(a), Path::new(b)));
        assert_eq!(
            paths,
            [
                "Vol 1/intro.png",
                "Vol 1/zz.png",
                "Vol 1/Ch 1/1.png",
                "Vol 1/Ch 2/1.png"
            ]
        );
    }

    #[test]
    fn a_file_comes_before_a_folder_even_when_it_sorts_after_it_by_name() {
        assert_eq!(
            compare_paths(Path::new("z.png"), Path::new("a/1.png")),
            Ordering::Less
        );
        assert_eq!(
            compare_paths(Path::new("a/1.png"), Path::new("z.png")),
            Ordering::Greater
        );
    }

    #[test]
    fn files_among_themselves_and_folders_among_themselves_keep_the_natural_order() {
        assert_eq!(
            compare_paths(Path::new("page2.png"), Path::new("page10.png")),
            Ordering::Less
        );
        assert_eq!(
            compare_paths(Path::new("Ch 2/1.png"), Path::new("Ch 10/1.png")),
            Ordering::Less
        );
    }

    #[test]
    fn equal_paths_compare_equal_and_a_prefix_comes_first() {
        assert_eq!(
            compare_paths(Path::new("a/1.png"), Path::new("a/1.png")),
            Ordering::Equal
        );
        assert_eq!(
            compare_paths(Path::new("a"), Path::new("a/1.png")),
            Ordering::Less
        );
    }
}
