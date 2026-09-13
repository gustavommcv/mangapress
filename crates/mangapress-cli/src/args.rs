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

    /// How aggressively margin/page-number cropping detects content —
    /// higher crops through more.
    #[arg(long, default_value_t = 1.0)]
    pub croppingpower: f32,

    /// Only actually crop if doing so would keep at least this percentage
    /// (0-100) of the page's area.
    #[arg(long, default_value_t = 0.0)]
    pub croppingminimum: f32,

    /// Back off the computed crop by this percentage (0-100) after the 10%
    /// cap, so some margin is deliberately kept.
    #[arg(long, default_value_t = 0.0)]
    pub preservemargin: f32,

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

    /// Quantize to the device profile's grayscale palette (dithered) and
    /// save PNG instead of full-tone JPEG.
    #[arg(long)]
    pub forcepng: bool,

    /// Rotate double-page spreads clockwise instead of the default
    /// counter-clockwise.
    #[arg(long)]
    pub rotateright: bool,

    /// Apply gamma correction to linearize the image. Values below 0.1 mean
    /// "use the device profile's own gamma" (always 1.0, i.e. a no-op,
    /// today).
    #[arg(short, long)]
    pub gamma: Option<f32>,

    /// Set the most common dark pixel value as the black point before
    /// autocontrast.
    #[arg(long)]
    pub autolevel: bool,

    /// Disable autocontrast.
    #[arg(long)]
    pub noautocontrast: bool,

    /// Crop empty inter-panel sections.
    #[arg(long = "ipc", value_enum, default_value_t = InterPanelCrop::Disabled)]
    pub interpanelcrop: InterPanelCrop,

    /// Erase rainbow effect on color e-ink screens by attenuating
    /// interfering frequencies.
    #[arg(long)]
    pub eraserainbow: bool,

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

    /// How to use ComicInfo.xml's own Title field, if the source has one.
    #[arg(long, value_enum, default_value_t = MetadataTitle::SeriesOnly)]
    pub metadatatitle: MetadataTitle,

    /// Keep the source's ComicInfo.xml in CBZ output.
    #[arg(long)]
    pub keepcomicinfo: bool,

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

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InterPanelCrop {
    Disabled,
    Horizontal,
    Both,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetadataTitle {
    /// Use ComicInfo.xml's Series/Volume/Number only.
    SeriesOnly,
    /// Append ": Title" after Series/Volume/Number.
    Combine,
    /// Use ComicInfo.xml's Title alone, overriding even an explicit -t.
    TitleOnly,
}
