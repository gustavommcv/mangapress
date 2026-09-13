//! CLI surface for v1. Deliberately a subset of `kcc-c2e.py`'s full flag
//! list (see the KCC research notes for the complete upstream inventory) —
//! only what's needed for the EPUB/CBZ/PDF-out, `.cbz`/folder-in scope
//! this project actually targets. Flags are added as their backing feature
//! gets implemented in `mangapress-core`, not preemptively.

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "mangapress",
    version,
    about = "Manga/comic converter for e-ink readers"
)]
pub struct Cli {
    /// Path to a `.cbz` file or a folder of chapter subfolders/images.
    pub input: PathBuf,

    /// Device profile (e.g. KV, KPW5, KoAO, Rmk2, OTHER). See
    /// `mangapress-core::profile::PROFILES` for the full list.
    #[arg(short, long, default_value = "KV")]
    pub profile: String,

    /// Right-to-left reading order and spread-split order.
    #[arg(short = 'm', long = "manga-style")]
    pub manga_style: bool,

    /// Cropping mode.
    #[arg(short, long, value_enum, default_value_t = Cropping::MarginsAndPageNumbers)]
    pub cropping: Cropping,

    /// Double-page spread handling.
    #[arg(short = 'r', long, value_enum, default_value_t = Splitter::Split)]
    pub splitter: Splitter,

    /// Resize images smaller than the device's resolution.
    #[arg(short, long)]
    pub upscale: bool,

    /// Stretch images to the device's resolution, ignoring aspect ratio.
    #[arg(short, long)]
    pub stretch: bool,

    /// Crop to fill the screen (ignores --upscale).
    #[arg(long)]
    pub wallpaper: bool,

    /// Disable autodetection and force white borders instead of padding
    /// with the page's detected background color (CBZ/PDF output only).
    #[arg(long)]
    pub whiteborders: bool,

    /// Output format.
    #[arg(short, long, value_enum, default_value_t = Format::Epub)]
    pub format: Format,

    /// Output file or directory.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Comic/book title (default: derived from the input file/folder name).
    #[arg(short, long)]
    pub title: Option<String>,

    /// Author name.
    #[arg(short, long)]
    pub author: Option<String>,

    /// EPUB language code.
    #[arg(long, default_value = "en-US")]
    pub language: String,

    /// Replace the screen width provided by the device profile. Can be used
    /// with `--profile OTHER` for a fully custom resolution, or alongside a
    /// named profile to override just one dimension.
    #[arg(long)]
    pub customwidth: Option<u32>,

    /// Replace the screen height provided by the device profile.
    #[arg(long)]
    pub customheight: Option<u32>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cropping {
    Disabled,
    Margins,
    MarginsAndPageNumbers,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Splitter {
    Split,
    Rotate,
    Both,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Epub,
    Cbz,
    Pdf,
}
