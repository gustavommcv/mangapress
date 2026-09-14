use serde_json::{json, Map, Value};
use std::io::{self, Write};
use std::sync::Mutex;

pub const VERSION: u32 = 1;

struct EventState<W> {
    writer: W,
    sequence: u64,
}

/// Serializes one JSON object per line. The mutex gives concurrent page
/// workers a single ordered stream and keeps sequence numbers contiguous.
pub struct EventSink<W> {
    enabled: bool,
    state: Mutex<EventState<W>>,
}

impl<W: Write> EventSink<W> {
    pub fn new(enabled: bool, writer: W) -> Self {
        Self {
            enabled,
            state: Mutex::new(EventState {
                writer,
                sequence: 0,
            }),
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn emit(&self, event_type: &str, fields: Value) -> io::Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let mut state = self
            .state
            .lock()
            .map_err(|_| io::Error::other("JSON event writer lock was poisoned"))?;
        state.sequence += 1;

        let mut object = match fields {
            Value::Object(object) => object,
            Value::Null => Map::new(),
            _ => return Err(io::Error::other("JSON event fields must be an object")),
        };
        object.insert("protocol_version".to_string(), json!(VERSION));
        object.insert("tool".to_string(), json!("mangapress"));
        object.insert("tool_version".to_string(), json!(env!("CARGO_PKG_VERSION")));
        object.insert("sequence".to_string(), json!(state.sequence));
        object.insert("type".to_string(), json!(event_type));

        serde_json::to_writer(&mut state.writer, &Value::Object(object))?;
        state.writer.write_all(b"\n")?;
        state.writer.flush()
    }

    pub fn emit_failure(&self, failure: &RunFailure) -> io::Result<()> {
        let mut fields = Map::new();
        fields.insert("severity".to_string(), json!("error"));
        fields.insert("code".to_string(), json!(failure.code));
        fields.insert("stage".to_string(), json!(failure.stage));
        fields.insert("recoverable".to_string(), json!(failure.recoverable));
        fields.insert("message".to_string(), json!(failure.message));
        fields.insert("diagnostic".to_string(), json!(failure.diagnostic));
        insert_optional(&mut fields, "manga", failure.manga.as_deref());
        insert_optional(&mut fields, "volume", failure.volume.as_deref());
        insert_optional(&mut fields, "chapter", failure.chapter.as_deref());
        if let Some(page) = failure.page {
            fields.insert("page".to_string(), json!(page));
        }
        insert_optional(&mut fields, "path", failure.path.as_deref());
        self.emit("error", Value::Object(fields))
    }
}

fn insert_optional(object: &mut Map<String, Value>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        object.insert(key.to_string(), json!(value));
    }
}

#[derive(Clone, Debug)]
pub struct RunFailure {
    pub code: &'static str,
    pub stage: &'static str,
    pub recoverable: bool,
    pub message: String,
    pub diagnostic: String,
    pub manga: Option<String>,
    pub volume: Option<String>,
    pub chapter: Option<String>,
    pub page: Option<usize>,
    pub path: Option<String>,
}

impl RunFailure {
    pub fn new(
        code: &'static str,
        stage: &'static str,
        recoverable: bool,
        message: impl Into<String>,
        diagnostic: impl Into<String>,
    ) -> Self {
        Self {
            code,
            stage,
            recoverable,
            message: message.into(),
            diagnostic: diagnostic.into(),
            manga: None,
            volume: None,
            chapter: None,
            page: None,
            path: None,
        }
    }

    pub fn with_chapter(mut self, chapter: impl Into<String>) -> Self {
        self.chapter = Some(chapter.into());
        self
    }

    pub fn with_manga(mut self, manga: impl Into<String>) -> Self {
        self.manga = Some(manga.into());
        self
    }

    pub fn with_page(mut self, page: usize) -> Self {
        self.page = Some(page);
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }
}

impl std::fmt::Display for RunFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.diagnostic)
    }
}

impl std::error::Error for RunFailure {}

pub fn event_write_failure(error: io::Error) -> RunFailure {
    RunFailure::new(
        "event_write_failed",
        "protocol",
        false,
        "Couldn't write the machine-readable event stream.",
        format!("writing JSON event: {error}"),
    )
}
