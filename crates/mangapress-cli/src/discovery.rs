use std::path::{Path, PathBuf};

/// The spread labels upstream's "Label Spreads" window leaves beside a
/// source: a file named like the source plus `.json`.
pub(super) fn spread_labels_beside(input: &Path) -> Option<PathBuf> {
    // Rebuilt from its components so that a trailing separator on a folder
    // doesn't end up in the middle of the name.
    let mut name = input.components().collect::<PathBuf>().into_os_string();
    name.push(".json");
    let path = PathBuf::from(name);
    path.is_file().then_some(path)
}

/// The positions in a spread-label file: `{"spreads": [12, 40]}`.
pub(super) fn read_spread_labels(path: &Path) -> Result<Vec<usize>, String> {
    let bytes = mangapress_core::input::read_file(path).map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| error.to_string())?;
    let positions = value
        .get("spreads")
        .and_then(|spreads| spreads.as_array())
        .ok_or_else(|| "it has no \"spreads\" list".to_string())?;
    positions
        .iter()
        .map(|position| {
            position
                .as_u64()
                .map(|position| position as usize)
                .ok_or_else(|| format!("{position} is not a page position"))
        })
        .collect()
}

/// The folder a book's own cover is looked for in, beside the book.
pub(super) const COVERS_FOLDER: &str = "Covers";

/// The cover a `Covers` folder beside `input` holds for it, if any —
/// upstream's convention for giving each volume of a series its own cover
/// without naming one on the command line.
///
/// Upstream matches by position alone: the folder's Nth image, in natural
/// order, goes to the Nth book beside it. That is kept, with two changes:
/// - An image named like the book (`Vol 3.jpg` for `Vol 3.cbz`) is that
///   book's cover, wherever it sorts. And once any image in the folder is
///   named after a book, position is not used at all: a book without an
///   image of its own then has no custom cover, rather than the cover of
///   whichever book happens to line up with it.
/// - What counts as "a book beside it" is the input's own kind: files with
///   its extension, or — for a folder — the other folders, leaving out
///   `Covers` itself (which upstream counts, giving every folder that sorts
///   after it the next book's cover). Earlier conversions' output (`_kcc`
///   in the name, as upstream skips, and this tool's own ` (mangapress`)
///   doesn't count either.
pub(super) fn cover_by_convention(input: &Path) -> Option<PathBuf> {
    let input = std::path::absolute(input).ok()?;
    let parent = input.parent()?;
    let covers_dir = parent.join(COVERS_FOLDER);
    if !covers_dir.is_dir() {
        return None;
    }
    let names_in = |dir: &Path, keep: &dyn Fn(&Path) -> bool| -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| keep(path))
            .filter_map(|path| Some(path.file_name()?.to_string_lossy().into_owned()))
            .collect();
        names.sort_by(|a, b| mangapress_core::natural_sort::compare(a, b));
        names
    };

    let covers = names_in(&covers_dir, &|path| {
        path.is_file() && mangapress_core::archive::has_image_extension(path)
    });

    let is_folder = input.is_dir();
    let extension = input.extension().map(|e| e.to_ascii_lowercase());
    let books = names_in(parent, &|path| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if name.contains("_kcc") || name.contains(" (mangapress") {
            return false;
        }
        if is_folder {
            path.is_dir() && name != COVERS_FOLDER
        } else {
            path.is_file() && path.extension().map(|e| e.to_ascii_lowercase()) == extension
        }
    });

    // A folder's name is its title whole; a file's, without the extension.
    let book_title = |name: &str| -> String {
        if is_folder {
            name.to_lowercase()
        } else {
            Path::new(name)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase()
        }
    };
    let cover_title = |name: &str| -> String {
        Path::new(name)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    };

    let name = input.file_name()?.to_string_lossy().into_owned();
    let title = book_title(&name);
    if let Some(cover) = covers.iter().find(|cover| cover_title(cover) == title) {
        return Some(covers_dir.join(cover));
    }
    let named_after_books = covers.iter().any(|cover| {
        let cover = cover_title(cover);
        books.iter().any(|book| book_title(book) == cover)
    });
    if named_after_books {
        return None;
    }
    let position = books.iter().position(|book| *book == name)?;
    covers.get(position).map(|cover| covers_dir.join(cover))
}
