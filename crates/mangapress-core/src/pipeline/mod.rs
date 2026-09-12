//! Per-page processing orchestration — the equivalent of KCC's
//! `ComicPageParser`/`ComicPage` (`image.py`) and `imgFileProcessing()`
//! (`comic2ebook.py`).

pub mod page;
pub mod spread;

/// Options that drive a single conversion run. Mirrors the relevant subset
/// of `kcc-c2e.py`'s argument groups (MAIN/PROCESSING) — see
/// `mangapress-cli`'s `args.rs` for the full CLI surface and which of these
/// are already wired up vs still `todo!()` downstream.
#[derive(Debug, Clone)]
pub struct PipelineOptions {
    pub profile: &'static crate::profile::Profile,
    /// `--customwidth`/`--customheight`: independently override either
    /// dimension of `profile`'s resolution. See
    /// [`crate::profile::Profile::effective_resolution`].
    pub width_override: Option<u32>,
    pub height_override: Option<u32>,
    pub manga_style: bool,
    pub cropping: CroppingMode,
    pub cropping_power: f32,
    pub cropping_minimum: f32,
    pub inter_panel_crop: crate::crop::inter_panel::InterPanelMode,
    pub splitter: SplitterMode,
    pub upscale: bool,
    pub stretch: bool,
    pub gamma: Option<f32>,
}

impl PipelineOptions {
    /// The resolution pages are actually resized to, after applying any
    /// `--customwidth`/`--customheight` override on top of `profile`.
    pub fn target_resolution(&self) -> (u32, u32) {
        self.profile
            .effective_resolution(self.width_override, self.height_override)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CroppingMode {
    Disabled,
    Margins,
    MarginsAndPageNumbers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitterMode {
    Split,
    Rotate,
    Both,
}

/// Processes a single source page into one or more output pages (a spread
/// may become two split halves and/or a rotated whole — see [`spread`]),
/// running the full per-page pipeline on each: crop -> gamma -> grayscale ->
/// autocontrast -> resize -> rainbow removal -> quantize -> encode.
pub fn process_page(
    _source_bytes: &[u8],
    _options: &PipelineOptions,
) -> crate::error::Result<Vec<Vec<u8>>> {
    todo!("wire crop/contrast/resize/rainbow/quantize together per page — implement after each stage has its own fixture-backed tests")
}
