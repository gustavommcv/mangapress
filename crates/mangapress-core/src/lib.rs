//! Core conversion pipeline for mangapress.
//!
//! Module boundaries mirror the file-level separation of concerns found in
//! KCC's `kindlecomicconverter/` package (profiles / crop algorithms / resize
//! / archive I/O / ebook builders each isolated), which is worth preserving
//! independently of the language rewrite. See `docs/adr/` for the reasoning
//! behind each boundary and behavioral choice.

pub mod archive;
pub mod contrast;
pub mod crop;
pub mod ebook;
pub mod error;
pub mod manga;
pub mod metadata;
pub mod natural_sort;
pub mod pipeline;
pub mod profile;
pub mod quantize;
pub mod rainbow;
pub mod resize;

pub use error::{Error, Result};
pub use profile::Profile;
