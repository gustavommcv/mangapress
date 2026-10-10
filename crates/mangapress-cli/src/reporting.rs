use crate::protocol::{event_write_failure, EventSink};
use serde_json::json;
use std::path::Path;

pub(super) fn absolute_display(path: &Path) -> String {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
    .to_string_lossy()
    .into_owned()
}

/// `SystemTime` as `YYYY-MM-DDThh:mm:ssZ`, for the EPUB's required
/// `dcterms:modified`. Hand-rolled (days-since-epoch to a civil date) rather
/// than pulling in a date-time crate for this one field. A clock set before
/// 1970 reads as the epoch.
pub(super) fn utc_timestamp(now: std::time::SystemTime) -> String {
    let seconds = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    let (days, second_of_day) = (seconds / 86_400, seconds % 86_400);

    // Days since 1970-01-01 to a proleptic Gregorian date, counting in
    // 400-year eras that start on 1 March so the leap day falls last.
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let day_of_era = shifted % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        second_of_day / 3_600,
        second_of_day % 3_600 / 60,
        second_of_day % 60
    )
}

/// One recoverable problem, as a protocol event or a line on the terminal.
pub(super) fn warn<W: std::io::Write + Send>(
    events: &EventSink<W>,
    code: &str,
    stage: &str,
    path: &str,
    message: &str,
) -> anyhow::Result<()> {
    if events.enabled() {
        events
            .emit(
                "warning",
                json!({
                    "severity": "warning",
                    "code": code,
                    "stage": stage,
                    "path": path,
                    "recoverable": true,
                    "message": message,
                }),
            )
            .map_err(event_write_failure)?;
    } else {
        eprintln!("warning: {message}");
    }
    Ok(())
}

/// A human report may stop when its reader closes the pipe; other I/O failures remain errors.
pub(super) fn write_human_report<W: std::io::Write>(
    writer: &mut W,
    report: impl FnOnce(&mut W) -> std::io::Result<()>,
) -> std::io::Result<()> {
    match report(writer).and_then(|()| writer.flush()) {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}
