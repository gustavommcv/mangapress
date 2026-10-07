# Installer checks

Build the CLI, then run the standard-library tests from the repository root:

```sh
cargo build --workspace --locked
python -m unittest discover -s tools/installers -p 'test_*.py' -v
```

The tests run the actual installer scripts against generated archives containing the
local CLI and synthetic notice texts. Only downloads are mocked: SHA-256 checking,
extraction, executable version checking, file installation, and cleanup remain native.
No public release is downloaded or published. All installation paths are temporary;
Windows uses the documented PATH opt-out and checks that the user's PATH is unchanged.

Linux/macOS exercise POSIX `sh`. Windows exercises both Windows PowerShell 5.1 and
PowerShell 7. Other platforms' suites are explicitly skipped; a missing interpreter
or unbuilt CLI on the applicable platform is an error, not a silently skipped check.
CI runs these tests after the workspace tests, using their built CLI.
POSIX tests also isolate PATH to exercise the native `shasum` fallback and rejection
when neither hashing tool is available.

Cases cover release selection, invalid versions, checksum absence/ambiguity/malformed
data/tampering, failed downloads, missing executables, wrong executable versions,
legacy archives, notice preservation, updates, and temporary-file cleanup. They are not
an end-to-end audit of GitHub's release service or publisher authenticity.
