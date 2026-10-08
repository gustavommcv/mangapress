// Native link helpers shared by library and real-binary regression tests.
// Only Windows' missing symlink privilege is a supported reason to skip.

use std::path::Path;

pub fn supported(root: &Path) -> bool {
    let probe = root.join("link-probe");
    match file_link(Path::new("missing-probe-target"), &probe) {
        Ok(()) => {
            std::fs::remove_file(probe).unwrap();
            true
        }
        #[cfg(windows)]
        Err(error) if error.raw_os_error() == Some(1314) => {
            eprintln!(
                "symlink fixture unavailable: Windows requires Developer Mode or symlink privilege"
            );
            false
        }
        Err(error) => panic!("could not create a symbolic-link fixture: {error}"),
    }
}

pub fn file_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
}

pub fn directory_link(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
}

// Junctions exercise directory redirection on Windows without enabling
// Developer Mode. Use PowerShell's native operation, only in test fixtures.
#[cfg(windows)]
pub fn junction(target: &Path, link: &Path) {
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command",
            "New-Item -ItemType Junction -Path $env:MANGAPRESS_TEST_JUNCTION_LINK -Target $env:MANGAPRESS_TEST_JUNCTION_TARGET -ErrorAction Stop | Out-Null"])
        .env("MANGAPRESS_TEST_JUNCTION_LINK", link)
        .env("MANGAPRESS_TEST_JUNCTION_TARGET", target)
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
