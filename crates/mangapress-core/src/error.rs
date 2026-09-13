//! Library error type. `mangapress-core` is a library crate — it defines its
//! own error enum rather than using `anyhow`, which is reserved for the
//! `mangapress-cli` binary crate where error aggregation (not matching)
//! is all that's needed.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("image decode/encode error: {0}")]
    Image(#[from] image::ImageError),

    #[error("archive error: {0}")]
    Archive(#[from] zip::result::ZipError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unknown device profile code: {0}")]
    UnknownProfile(String),

    #[error("cannot build an ebook with no pages")]
    EmptyBook,

    #[error("PDF error: {0}")]
    Pdf(String),
}

pub type Result<T> = std::result::Result<T, Error>;
