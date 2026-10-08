mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use support::{binary, fixture_folder, parse_events, write_png};

fn cbz(root: &Path, name: &str, series: &str) -> PathBuf {
    let page_path = root.join("source.png");
    write_png(&page_path, 89);
    let entries = vec![
        ("c001/p1.png".to_owned(), fs::read(page_path).unwrap()),
        (
            "ComicInfo.xml".to_owned(),
            format!("<ComicInfo><Series>{series}</Series></ComicInfo>").into_bytes(),
        ),
    ];
    let input = root.join(name);
    fs::write(
        &input,
        mangapress_core::archive::cbz::write_zip(&entries, true).unwrap(),
    )
    .unwrap();
    input
}

#[test]
fn folders_with_dots_keep_their_whole_names_without_collisions() {
    let root = tempfile::tempdir().unwrap();
    for name in ["Vol. 1", "Vol. 2", "Vol 1.5"] {
        let input = root.path().join(name);
        fs::create_dir(&input).unwrap();
        write_png(&input.join("p1.png"), 89);
        let output = Command::new(binary())
            .arg(&input)
            .args(["--format", "cbz", "--noprocessing", "--json-events"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let result = events.last().unwrap();
        let path = Path::new(result["output_path"].as_str().unwrap());
        assert_eq!(path, root.path().join(format!("{name}.cbz")));
        assert!(path.exists());
        assert!(!events.iter().any(|event| event["code"] == "output_exists"));
    }
}

#[test]
fn dot_and_parent_inputs_resolve_to_the_real_folder_name() {
    for argument in [".", ".."] {
        let root = tempfile::tempdir().unwrap();
        let input = root.path().join("Vol. 1");
        fs::create_dir(&input).unwrap();
        let child = input.join("child");
        fs::create_dir(&child).unwrap();
        write_png(&input.join("p1.png"), 89);
        let cwd = if argument == "." { &input } else { &child };
        let output = Command::new(binary())
            .current_dir(cwd)
            .arg(argument)
            .args(["--format", "cbz", "--noprocessing", "--json-events"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let path = Path::new(events.last().unwrap()["output_path"].as_str().unwrap());
        assert_eq!(path.file_name().unwrap(), "Vol. 1.cbz");
        assert_eq!(
            fs::canonicalize(path.parent().unwrap()).unwrap(),
            fs::canonicalize(root.path()).unwrap()
        );
        assert!(path.exists());
        assert_eq!(
            fs::read_dir(&input).unwrap().count(),
            2,
            "no output or staging file inside the input"
        );
    }
}

#[test]
fn identical_metadata_and_long_titles_do_not_choose_the_output_filename() {
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("books");
    fs::create_dir(&destination).unwrap();
    let series = "A".repeat(350);
    for name in ["Vol.01.cbz", "Vol.02.cbz"] {
        let input = cbz(root.path(), name, &series);
        let output = Command::new(binary())
            .arg(input)
            .args([
                "--format",
                "cbz",
                "--noprocessing",
                "--keepcomicinfo",
                "--json-events",
                "--output",
            ])
            .arg(&destination)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = parse_events(&output);
        let result = events.last().unwrap();
        assert_eq!(result["manga"], series);
        let path = Path::new(result["output_path"].as_str().unwrap());
        assert_eq!(path, destination.join(name));
        let mut archive = zip::ZipArchive::new(fs::File::open(path).unwrap()).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut archive.by_name("ComicInfo.xml").unwrap(), &mut xml)
            .unwrap();
        assert!(xml.contains(&series));
    }
    assert_eq!(fs::read_dir(destination).unwrap().count(), 2);
}

#[test]
fn an_explicit_existing_book_is_preserved_and_the_actual_name_is_reported() {
    for machine in [false, true] {
        let input = fixture_folder();
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("book.epub");
        fs::write(&destination, b"previous book").unwrap();
        let mut command = Command::new(binary());
        command
            .arg(input.path())
            .args(["--noprocessing", "--output"])
            .arg(&destination);
        if machine {
            command.arg("--json-events");
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fs::read(&destination).unwrap(), b"previous book");
        let alternative = root.path().join("book (mangapress).epub");
        assert!(alternative.exists());
        if machine {
            let events = parse_events(&output);
            let warning = events
                .iter()
                .find(|event| event["code"] == "output_exists")
                .unwrap();
            assert_eq!(warning["stage"], "plan");
            assert_eq!(Path::new(warning["path"].as_str().unwrap()), alternative);
            let result = events.last().unwrap();
            assert_eq!(result["type"], "result");
            assert_eq!(
                Path::new(result["output_path"].as_str().unwrap()),
                alternative
            );
        } else {
            assert!(output.stdout.is_empty());
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("already exists"));
            assert!(stderr.contains("book (mangapress).epub"));
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }
}

#[test]
fn an_explicit_alias_of_the_input_keeps_the_existing_protocol_code() {
    let root = tempfile::tempdir().unwrap();
    let input = cbz(root.path(), "Vol.01.cbz", "Series");
    let original = fs::read(&input).unwrap();
    let output = Command::new(binary())
        .arg(&input)
        .args([
            "--format",
            "cbz",
            "--noprocessing",
            "--json-events",
            "--output",
        ])
        .arg(root.path().join(".").join("Vol.01.cbz"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let events = parse_events(&output);
    assert!(events
        .iter()
        .any(|event| event["code"] == "output_collision"));
    assert!(!events.iter().any(|event| event["code"] == "output_exists"));
    assert_eq!(fs::read(input).unwrap(), original);
    assert!(root.path().join("Vol.01 (mangapress).cbz").exists());
}

#[test]
fn missing_parents_are_planned_without_writes_then_created_for_a_real_run() {
    for as_directory in [false, true] {
        let input = fixture_folder();
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("missing/nested");
        let destination = if as_directory {
            directory.clone()
        } else {
            directory.join("book.epub")
        };
        let mut planned = None;
        for dry_run in [true, false] {
            let mut command = Command::new(binary());
            command
                .arg(input.path())
                .args(["--noprocessing", "--json-events", "--output"])
                .arg(&destination);
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let events = parse_events(&output);
            let result = events.last().unwrap();
            let actual = PathBuf::from(result["output_path"].as_str().unwrap());
            if dry_run {
                planned = Some(actual);
                assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
                assert!(!events
                    .iter()
                    .any(|event| event["stage"] == "process" || event["stage"] == "write"));
            } else {
                assert_eq!(&actual, planned.as_ref().unwrap());
                assert!(actual.exists());
                assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
            }
        }
    }
}

#[test]
fn impossible_output_paths_fail_before_page_work_even_on_a_dry_run() {
    let input = fixture_folder();
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("blocked");
    fs::write(&blocked, b"not a directory").unwrap();
    for path in [
        blocked.join("book.epub"),
        root.path().join(format!("{}.epub", "x".repeat(300))),
        root.path().join("CON.epub"),
        root.path().join("missing?/book.epub"),
    ] {
        for dry_run in [true, false] {
            let mut command = Command::new(binary());
            command
                .arg(input.path())
                .args(["--json-events", "--output"])
                .arg(&path);
            if dry_run {
                command.arg("--dry-run");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1), "{path:?}");
            let events = parse_events(&output);
            let error = events.last().unwrap();
            assert_eq!(error["type"], "error");
            assert_eq!(error["code"], "output_plan_failed", "{error}");
            assert_eq!(error["stage"], "plan");
            assert!(!events.iter().any(|event| event["type"] == "result"
                || event["stage"] == "process"
                || event["stage"] == "package"));
            assert_eq!(fs::read(&blocked).unwrap(), b"not a directory");
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
        }
    }
}

#[test]
fn a_processing_failure_removes_staging_and_preserves_the_existing_book() {
    let input = tempfile::tempdir().unwrap();
    fs::write(input.path().join("p1.png"), b"not an image").unwrap();
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("book.epub");
    fs::write(&destination, b"previous book").unwrap();
    let output = Command::new(binary())
        .arg(input.path())
        .args(["--json-events", "--output"])
        .arg(&destination)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let events = parse_events(&output);
    assert_eq!(events.last().unwrap()["code"], "page_processing_failed");
    assert!(!events.iter().any(|event| event["type"] == "result"));
    assert_eq!(fs::read(destination).unwrap(), b"previous book");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[cfg(unix)]
#[test]
fn output_permissions_follow_umask_for_all_formats_and_destinations() {
    use std::os::unix::fs::PermissionsExt;

    let input = fixture_folder();
    for (umask, expected) in [("022", 0o644), ("027", 0o640), ("077", 0o600)] {
        for format in ["epub", "cbz", "pdf"] {
            for as_directory in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let directory = root.path().join("books");
                fs::create_dir(&directory).unwrap();
                let destination = if as_directory {
                    directory.clone()
                } else {
                    directory.join(format!("book.{format}"))
                };
                // Set umask only in the child, never in the parallel test process.
                // Paths are arguments, not interpolated into the shell program.
                let output = Command::new("sh")
                    .args([
                        "-c",
                        "umask \"$1\"; shift; exec \"$@\"",
                        "mangapress-output-permissions",
                        umask,
                    ])
                    .arg(binary())
                    .arg(input.path())
                    .args([
                        "--format",
                        format,
                        "--noprocessing",
                        "--json-events",
                        "--output",
                    ])
                    .arg(&destination)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{format}, umask {umask}, directory {as_directory}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                let events = parse_events(&output);
                let result = events.last().unwrap();
                assert_eq!(result["type"], "result");
                let path = Path::new(result["output_path"].as_str().unwrap());
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    expected,
                    "{format}, umask {umask}, directory {as_directory}"
                );
                assert_eq!(path.parent().unwrap(), directory);
                assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
            }
        }
    }
}
