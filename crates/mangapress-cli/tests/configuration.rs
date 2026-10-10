mod support;

use std::process::Command;
use support::{binary, fixture_folder, parse_events};

#[test]
fn configuration_errors_keep_precedence_before_inspection_or_output_staging() {
    let input = fixture_folder();
    let missing = input.path().join("missing");
    let destination = tempfile::tempdir().unwrap();
    for (path, flags, code, stage, diagnostic) in [
        (
            missing.as_path(),
            vec!["--profile", "k11"],
            "unknown_profile",
            "configuration",
            "did you mean 'K11'",
        ),
        (
            missing.as_path(),
            vec!["--profile", "OTHER", "--format", "cbz", "--nested-toc"],
            "input_not_found",
            "inspect",
            "input path does not exist",
        ),
        (
            missing.as_path(),
            vec!["--customwidth", "0"],
            "input_not_found",
            "inspect",
            "input path does not exist",
        ),
        (
            input.path(),
            vec![
                "--profile",
                "OTHER",
                "--customwidth",
                "120",
                "--format",
                "cbz",
                "--nested-toc",
            ],
            "invalid_resolution",
            "configuration",
            "resolved target resolution is 120x0",
        ),
        (
            input.path(),
            vec!["--customheight", "0", "--format", "pdf", "--nested-toc"],
            "invalid_resolution",
            "configuration",
            "resolved target resolution is 1072x0",
        ),
        (
            input.path(),
            vec!["--profile", "KDX", "--nested-toc"],
            "nested_toc_unsupported_format",
            "configuration",
            "--nested-toc requires --format epub, got --format Cbz",
        ),
    ] {
        for machine in [false, true] {
            let mut command = Command::new(binary());
            command
                .arg(path)
                .args(&flags)
                .arg("--output")
                .arg(destination.path().join("not-created/book.epub"));
            if machine {
                command.arg("--json-events");
            }
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1));
            assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
            if machine {
                let events = parse_events(&output);
                assert_eq!(
                    events.len(),
                    1,
                    "no inspection/progress events on invalid configuration"
                );
                assert_eq!(events[0]["type"], "error");
                assert_eq!(events[0]["code"], code);
                assert_eq!(events[0]["stage"], stage);
                assert_eq!(events[0]["recoverable"], true);
                assert!(events[0]["diagnostic"]
                    .as_str()
                    .unwrap()
                    .contains(diagnostic));
            } else {
                assert!(output.stdout.is_empty());
            }
            assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 0);
        }
    }
}

#[test]
fn reporting_commands_bypass_conversion_configuration() {
    let directory = tempfile::tempdir().unwrap();
    for flags in [
        vec!["--list-profiles"],
        vec!["--list-profiles", "--json-events"],
        vec!["--protocol-version"],
    ] {
        let output = Command::new(binary())
            .arg(directory.path().join("missing"))
            .args([
                "--profile",
                "BOGUS",
                "--customwidth",
                "0",
                "--format",
                "pdf",
                "--nested-toc",
                "--output",
            ])
            .arg(directory.path().join("not-created/book.epub"))
            .args(&flags)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        if flags.contains(&"--protocol-version") || flags.contains(&"--json-events") {
            let events = parse_events(&output);
            assert!(events.iter().all(|event| matches!(
                event["type"].as_str(),
                Some("profile" | "protocol" | "result")
            )));
        } else {
            assert!(String::from_utf8_lossy(&output.stdout).contains("Kindle"));
        }
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
