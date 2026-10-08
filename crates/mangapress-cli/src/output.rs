//! Output planning and publication, independently of image processing.
//! Naming follows KCC 12.0.0's `getOutputFilename` behavior: source names,
//! whole folder names, and no replacement of an existing destination.
//! Portable names and publication guarantees are recorded in ADR 0016.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

// Common filesystem component limit. Count UTF-8 bytes conservatively so a
// generated filename also fits Windows' UTF-16 component limit.
const MAX_NAME_BYTES: usize = 255;

pub struct Plan {
    pub path: PathBuf,
    pub collision: bool,
    pub input_collision: bool,
}

/// Resolve the destination and check what can be checked without creating
/// anything. Actual writability is checked by staging before page work starts.
pub fn plan(input: &Path, output: Option<&Path>, extension: &str, kepub: bool) -> io::Result<Plan> {
    // Resolve . and .. before taking a name, without changing the name of an
    // ordinary symlink argument. The input has already been validated/read.
    let source = if input.file_name().is_none() {
        fs::canonicalize(input)?
    } else {
        std::path::absolute(input)?
    };
    let derived_name = || {
        let source_name = if input.is_dir() {
            source.file_name()
        } else {
            source.file_stem()
        }
        .ok_or_else(|| invalid("the input has no filename; choose an output file"))?
        .to_string_lossy();
        let stem = if output.is_none() && kepub && input.is_file() {
            kobo_safe_stem(&source_name)
        } else {
            source_name.into_owned()
        };
        generated_name(&stem, "", extension)
    };
    let requested = match output {
        Some(path) if path.is_dir() || (!occupied(path)? && !has_known_extension(path)) => {
            path.join(derived_name()?)
        }
        Some(path) => path.to_path_buf(),
        None => source.with_file_name(derived_name()?),
    };
    let requested = std::path::absolute(requested)?;
    validate_filename(&requested)?;
    let collision = occupied(&requested)?;
    let input_collision = collision
        && fs::canonicalize(input)
            .ok()
            .zip(fs::canonicalize(&requested).ok())
            .is_some_and(|(input, output)| input == output);
    let path = if collision {
        alternate_path(&requested)?
    } else {
        requested
    };
    validate_parent(&path)?;
    Ok(Plan {
        path,
        collision,
        input_collision,
    })
}

/// A private sibling file: the final path is not opened until all bytes have
/// been written and synchronized. Drop cleans up ordinary error paths.
pub struct StagedOutput {
    file: NamedTempFile,
    destination: PathBuf,
}

impl StagedOutput {
    /// The caller creates missing parents first, so directory-creation failures
    /// can keep their existing machine-protocol code.
    pub fn new(destination: &Path) -> io::Result<Self> {
        let file = tempfile::Builder::new()
            .prefix(".mangapress-")
            .suffix(".tmp")
            .tempfile_in(parent(destination)?)?;
        Ok(Self {
            file,
            destination: destination.to_path_buf(),
        })
    }

    pub fn write(self, bytes: &[u8]) -> io::Result<()> {
        self.publish(|file| file.write_all(bytes))
    }

    fn publish(mut self, write: impl FnOnce(&mut File) -> io::Result<()>) -> io::Result<()> {
        write(self.file.as_file_mut())?;
        self.file.as_file().sync_all()?;
        // Do not turn a check-then-write race into an overwrite. The crate
        // provides the platform operation and keeps the file on an error;
        // dropping that error's file removes our temporary, not the winner.
        self.file
            .persist_noclobber(&self.destination)
            .map_err(|error| error.error)?;
        Ok(())
    }
}

pub fn parent(path: &Path) -> io::Result<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid("the output must have a parent directory"))
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn occupied(path: &Path) -> io::Result<bool> {
    // Unlike exists(), this also preserves dangling symlinks and reports
    // permission/invalid-path errors instead of treating them as free names.
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn has_known_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("epub" | "cbz" | "pdf")
    )
}

fn reserved_name(name: &str) -> bool {
    let base = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    let number = base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"));
    number.is_some_and(|number| {
        matches!(
            number,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

fn illegal_character(character: char) -> bool {
    character.is_control()
        || matches!(
            character,
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        )
}

fn portable_stem(stem: &str, budget: usize) -> String {
    let cleaned: String = stem
        .chars()
        .filter(|c| !c.is_control())
        .map(|c| if illegal_character(c) { '-' } else { c })
        .collect();
    let mut cleaned = cleaned.trim_end_matches(['.', ' ']).to_owned();
    if cleaned.is_empty() {
        cleaned = "book".to_owned();
    }
    if reserved_name(&cleaned) {
        cleaned.insert(0, '_');
    }
    if cleaned.len() > budget {
        let mut end = budget;
        while !cleaned.is_char_boundary(end) {
            end -= 1;
        }
        cleaned.truncate(end);
    }
    let cleaned = cleaned.trim_end_matches(['.', ' ']);
    if cleaned.is_empty() || reserved_name(cleaned) {
        "book".to_owned()
    } else {
        cleaned.to_owned()
    }
}

fn generated_name(stem: &str, suffix: &str, extension: &str) -> io::Result<String> {
    let extension = if extension.is_empty() {
        String::new()
    } else {
        format!(".{extension}")
    };
    let budget = MAX_NAME_BYTES
        .checked_sub(suffix.len() + extension.len())
        .filter(|budget| *budget >= 4)
        .ok_or_else(|| invalid("the output extension leaves no room for an alternate filename"))?;
    let stem = portable_stem(stem, budget);
    Ok(format!("{stem}{suffix}{extension}"))
}

fn alternate_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("the output filename must be valid UTF-8"))?;
    // Keep a compound Kobo extension intact, rather than inserting the
    // collision suffix between `kepub` and `epub`.
    let (stem, extension) = if name.to_ascii_lowercase().ends_with(".kepub.epub") {
        (
            &name[..name.len() - ".kepub.epub".len()],
            &name[name.len() - "kepub.epub".len()..],
        )
    } else {
        (
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(name),
            path.extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or(""),
        )
    };
    for number in 1u64.. {
        let suffix = if number == 1 {
            " (mangapress)".to_owned()
        } else {
            format!(" (mangapress {number})")
        };
        let candidate = path.with_file_name(generated_name(stem, &suffix, extension)?);
        if !occupied(&candidate)? {
            return Ok(candidate);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free output filename",
    ))
}

fn validate_filename(path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("the output must have a filename"))?;
    let name = name
        .to_str()
        .ok_or_else(|| invalid("the output path must be valid UTF-8"))?;
    if name.len() > MAX_NAME_BYTES
        || name.chars().any(illegal_character)
        || name.ends_with(['.', ' '])
        || reserved_name(name)
    {
        return Err(invalid(&format!("invalid output path component {name:?}: use at most {MAX_NAME_BYTES} UTF-8 bytes, no control/reserved characters or device names, and no trailing dot or space")));
    }
    Ok(())
}

fn validate_parent(path: &Path) -> io::Result<()> {
    let mut directory = parent(path)?;
    loop {
        match fs::metadata(directory) {
            Ok(metadata) => {
                if !metadata.is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        format!("output parent '{}' is not a directory", directory.display()),
                    ));
                }
                // Windows' directory read-only attribute is not a write
                // permission. ACLs are checked by actual staging, not inferred.
                if cfg!(unix) && metadata.permissions().readonly() {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("output parent '{}' is read-only", directory.display()),
                    ));
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                validate_filename(directory)?;
                directory = parent(directory)?;
            }
            Err(error) => return Err(error),
        }
    }
}

// Existing Kobo filename behavior; not used for explicitly named outputs.
fn kobo_safe_stem(stem: &str) -> String {
    let mut out = String::with_capacity(stem.len());
    let mut in_run = false;
    for c in stem.chars() {
        if c.is_alphanumeric() || c == '_' {
            out.push(c);
            in_run = false;
        } else if !in_run {
            out.push('_');
            in_run = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(root: &Path, name: &str) -> PathBuf {
        let path = root.join(name);
        fs::write(&path, b"source").unwrap();
        path
    }

    #[test]
    fn known_extensions_keep_the_existing_file_or_directory_convention() {
        for name in ["book.epub", "book.EPUB", "book.cbz", "book.pdf"] {
            assert!(has_known_extension(Path::new(name)));
        }
        for name in ["book.zip", "output-folder"] {
            assert!(!has_known_extension(Path::new(name)));
        }
    }

    #[test]
    fn kobo_names_keep_the_existing_word_character_rule() {
        assert_eq!(kobo_safe_stem("My Book (v1)"), "My_Book_v1_");
        assert_eq!(kobo_safe_stem("already_safe_01"), "already_safe_01");
        assert_eq!(kobo_safe_stem("君の名は - 1"), "君の名は_1");
    }

    #[test]
    fn generated_stems_are_portable_without_modifying_book_metadata() {
        for (input, expected) in [
            (r#"a/b\c:d*e?f"g<h>i|j"#, "a-b-c-d-e-f-g-h-i-j"),
            ("Chainsaw Man - Vol.01", "Chainsaw Man - Vol.01"),
            ("line\nbreak\t. ", "linebreak"),
            ("..", "book"),
            ("", "book"),
            ("CON", "_CON"),
            ("nul.extra", "_nul.extra"),
            ("COM1", "_COM1"),
            ("LPT³", "_LPT³"),
            ("COM10", "COM10"),
        ] {
            assert_eq!(portable_stem(input, MAX_NAME_BYTES), expected, "{input:?}");
        }
        assert_eq!(portable_stem("LPT1-long", 4), "book");
    }

    #[test]
    fn unicode_names_fit_the_budget_with_the_suffix_and_compound_extension() {
        for suffix in ["", " (mangapress)", " (mangapress 18446744073709551615)"] {
            let name = generated_name(&"漫画".repeat(100), suffix, "kepub.epub").unwrap();
            assert!(name.len() <= MAX_NAME_BYTES);
            assert!(name.ends_with(&format!("{suffix}.kepub.epub")));
            assert!(validate_filename(Path::new(&name)).is_ok());
        }
        assert!(generated_name("book", " (mangapress)", &"x".repeat(250)).is_err());
    }

    #[test]
    fn explicit_invalid_names_are_rejected_not_silently_rewritten() {
        for name in [
            "CON.epub",
            "nul.extra.epub",
            "COM¹.epub",
            "trailing. ",
            "tab\t.epub",
            "question?.epub",
        ] {
            assert_eq!(
                validate_filename(Path::new(name)).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
        }
        assert!(validate_filename(Path::new(&format!("{}.epub", "x".repeat(255)))).is_err());
    }

    #[test]
    fn whole_folder_names_keep_their_dots() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("Vol. 1.5");
        fs::create_dir(&input).unwrap();
        let result = plan(&input, None, "epub", false).unwrap();
        assert_eq!(result.path, root.path().join("Vol. 1.5.epub"));
        assert!(!result.collision);
    }

    #[test]
    fn an_explicit_filename_does_not_require_an_input_basename() {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().ancestors().last().unwrap();
        let output = root.path().join("book.epub");
        assert_eq!(
            plan(input, Some(&output), "epub", false).unwrap().path,
            output
        );
        assert!(plan(input, None, "epub", false).is_err());
    }

    #[test]
    fn a_file_uses_its_source_stem_and_preserves_the_input_under_an_alias() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "book.cbz");
        let alias = root.path().join(".").join("book.cbz");
        let result = plan(&input, Some(&alias), "cbz", false).unwrap();
        assert!(result.collision && result.input_collision);
        assert_eq!(result.path, root.path().join("book (mangapress).cbz"));
        assert_eq!(fs::read(input).unwrap(), b"source");
    }

    #[test]
    fn any_existing_entry_causes_an_alternate_name_with_counting() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let output = source(root.path(), "book.epub");
        fs::create_dir(root.path().join("book (mangapress).epub")).unwrap();
        let result = plan(&input, Some(&output), "epub", false).unwrap();
        assert!(result.collision && !result.input_collision);
        assert_eq!(result.path, root.path().join("book (mangapress 2).epub"));
        assert_eq!(fs::read(output).unwrap(), b"source");
    }

    #[test]
    fn a_kobo_collision_keeps_the_entire_kepub_extension() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let output = source(root.path(), "book.kepub.epub");
        let result = plan(&input, Some(&output), "kepub.epub", true).unwrap();
        assert_eq!(
            result.path,
            root.path().join("book (mangapress).kepub.epub")
        );
    }

    #[test]
    fn planning_nested_parents_is_read_only_and_blocked_parents_fail_early() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let output = root.path().join("missing/nested/book.epub");
        assert_eq!(
            plan(&input, Some(&output), "epub", false).unwrap().path,
            output
        );
        assert!(!root.path().join("missing").exists());
        let blocked = root.path().join("input.cbz/book.epub");
        assert!(plan(&input, Some(&blocked), "epub", false).is_err());
    }

    #[test]
    fn staging_publishes_only_a_complete_book_and_leaves_no_temporary() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("book.epub");
        let staged = StagedOutput::new(&destination).unwrap();
        assert!(!destination.exists());
        assert!(staged.file.path().starts_with(root.path()));
        staged.write(b"complete book").unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"complete book");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_partial_write_failure_preserves_existing_bytes_and_removes_our_temporary() {
        let root = tempfile::tempdir().unwrap();
        let destination = source(root.path(), "book.epub");
        let staged = StagedOutput::new(&destination).unwrap();
        let error = staged
            .publish(|file| {
                file.write_all(b"partial book")?;
                assert_eq!(fs::read(&destination)?, b"source");
                Err(io::Error::other("simulated write failure"))
            })
            .unwrap_err();
        assert_eq!(error.to_string(), "simulated write failure");
        assert_eq!(fs::read(&destination).unwrap(), b"source");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn abandoning_staging_cleans_up_without_creating_a_book() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("book.epub");
        let staged = StagedOutput::new(&destination).unwrap();
        let temporary = staged.file.path().to_path_buf();
        drop(staged);
        assert!(!temporary.exists());
        assert!(!destination.exists());
    }

    #[test]
    fn a_destination_created_after_planning_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("book.epub");
        let staged = StagedOutput::new(&destination).unwrap();
        fs::write(&destination, b"another process's book").unwrap();
        assert_eq!(
            staged.write(b"our book").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&destination).unwrap(), b"another process's book");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[test]
    fn simultaneous_publication_never_replaces_the_winner() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("book.epub");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = [b"first book".as_slice(), b"second book".as_slice()]
            .into_iter()
            .map(|bytes| {
                let staged = StagedOutput::new(&destination).unwrap();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    (bytes, staged.write(bytes))
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        let winners: Vec<_> = results
            .iter()
            .filter(|(_, result)| result.is_ok())
            .collect();
        assert_eq!(winners.len(), 1);
        let losers: Vec<_> = results
            .iter()
            .filter_map(|(_, result)| result.as_ref().err())
            .collect();
        assert_eq!(losers.len(), 1);
        assert_eq!(losers[0].kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&destination).unwrap(), winners[0].0);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_directory_read_only_attribute_is_not_treated_as_an_acl() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let directory = root.path().join("books");
        fs::create_dir(&directory).unwrap();
        let original = fs::metadata(&directory).unwrap().permissions();
        let mut flagged = original.clone();
        flagged.set_readonly(true);
        fs::set_permissions(&directory, flagged).unwrap();
        let result = (|| -> io::Result<()> {
            let plan = plan(&input, Some(&directory), "epub", false)?;
            StagedOutput::new(&plan.path)?.write(b"complete book")
        })();
        fs::set_permissions(&directory, original).unwrap();
        result.unwrap();
        assert_eq!(
            fs::read(directory.join("input.epub")).unwrap(),
            b"complete book"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_output_link_is_not_treated_as_a_free_name() {
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let destination = root.path().join("book.epub");
        std::os::unix::fs::symlink(root.path().join("missing-target"), &destination).unwrap();
        let result = plan(&input, Some(&destination), "epub", false).unwrap();
        assert!(result.collision);
        assert_eq!(result.path, root.path().join("book (mangapress).epub"));
        assert!(fs::symlink_metadata(destination)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_parent_is_rejected_without_creating_anything() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let input = source(root.path(), "input.cbz");
        let directory = root.path().join("read-only");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555)).unwrap();
        let result = plan(
            &input,
            Some(&directory.join("missing/book.epub")),
            "epub",
            false,
        );
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            result.err().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    }
}
