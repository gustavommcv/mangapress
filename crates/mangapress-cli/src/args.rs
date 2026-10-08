//! The command line. KCC 12.0.0's own options, under KCC's names where they
//! mean the same thing, minus the ones left out on purpose (MOBI, Panel
//! View, other input formats, volume grouping and splitting — the list and
//! the reasons are in `docs/adr/0013-follow-a-named-kcc-release.md`), plus
//! this tool's own (`--json-events`, `--dry-run`, `--nested-toc`, `--cover`,
//! `--list-profiles`).

use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "mangapress",
    version,
    about = "Manga/comic converter for e-ink readers",
    override_usage = "mangapress [OPTIONS] <INPUT>\n       mangapress --list-profiles [--json-events]\n       mangapress --protocol-version",
    after_help = concat!(
        "Examples:\n",
        "  mangapress \"Volume 1.cbz\" --profile K11 --format epub\n",
        "  mangapress \"Volume 1.cbz\" --profile K11 --dry-run\n",
        "  mangapress --list-profiles\n\n",
        "Exit codes:\n",
        "  0  Success\n",
        "  1  Runtime failure (configuration, input, conversion, output, or protocol)\n",
        "  2  Invalid arguments\n\n",
        "Documentation: ", env!("CARGO_PKG_REPOSITORY"), "#readme\n",
        "Report a problem: ", env!("CARGO_PKG_REPOSITORY"), "/issues",
    )
)]
pub struct Cli {
    /// Path to a `.cbz` file or a folder of chapter subfolders/images.
    /// Required unless --list-profiles or --protocol-version is passed.
    #[arg(required_unless_present_any = ["list_profiles", "protocol_version"])]
    pub input: Option<PathBuf>,

    /// Device profile (e.g. KV, KPW5, KoAO, Rmk2, OTHER). Run
    /// --list-profiles for the full list.
    #[arg(
        short,
        long,
        default_value = "KV",
        help_heading = "Conversion",
        display_order = 0
    )]
    pub profile: String,

    /// Print every supported device profile code, display name, and
    /// resolution, then exit.
    #[arg(long, help_heading = "Reporting")]
    pub list_profiles: bool,

    /// Emit a versioned JSON Lines event stream on stdout instead of
    /// human-readable progress output.
    #[arg(long, help_heading = "Reporting")]
    pub json_events: bool,

    /// Print machine-protocol compatibility information as JSON and exit.
    #[arg(long, help_heading = "Reporting")]
    pub protocol_version: bool,

    /// Show what would be produced (chapters, pages, resolved metadata,
    /// output path) without processing any pages or writing anything.
    #[arg(short = 'n', long = "dry-run", help_heading = "Reporting")]
    pub dry_run: bool,

    /// Suppress routine progress messages; warnings and errors still print.
    #[arg(short, long, help_heading = "Reporting")]
    pub quiet: bool,

    /// Right-to-left reading order and spread-split order.
    #[arg(short = 'm', long = "manga-style", help_heading = "Page layout")]
    pub manga_style: bool,

    /// Cropping mode.
    #[arg(short, long, value_enum, default_value_t = Cropping::MarginsAndPageNumbers, help_heading = "Cropping and sizing")]
    pub cropping: Cropping,

    /// How aggressively margin/page-number cropping detects content —
    /// higher crops through more.
    #[arg(long, default_value_t = 1.0, help_heading = "Cropping and sizing")]
    pub croppingpower: f32,

    /// Only actually crop if doing so would keep at least this percentage
    /// (0-100) of the page's area.
    #[arg(long, default_value_t = 0.0, help_heading = "Cropping and sizing")]
    pub croppingminimum: f32,

    /// Back off the computed crop by this percentage (0-100) after the 10%
    /// cap, so some margin is deliberately kept.
    #[arg(long, default_value_t = 0.0, help_heading = "Cropping and sizing")]
    pub preservemargin: f32,

    /// Double-page spread handling.
    #[arg(short = 'r', long, value_enum, default_value_t = Splitter::Split, help_heading = "Page layout")]
    pub splitter: Splitter,

    /// Resize images smaller than the device's resolution.
    #[arg(short, long, help_heading = "Cropping and sizing")]
    pub upscale: bool,

    /// Stretch images to the device's resolution, ignoring aspect ratio.
    #[arg(short, long, help_heading = "Cropping and sizing")]
    pub stretch: bool,

    /// Crop to fill the screen (ignores --upscale).
    #[arg(long, help_heading = "Cropping and sizing")]
    pub wallpaper: bool,

    /// Disable autodetection and force black borders: the pad color for
    /// CBZ/PDF pages and the page background in an EPUB.
    #[arg(long, help_heading = "Image quality")]
    pub blackborders: bool,

    /// Disable autodetection and force white borders: CBZ/PDF pages are not
    /// padded with the page's detected background color, and the page
    /// background in an EPUB is white even for a dark page.
    #[arg(long, help_heading = "Image quality")]
    pub whiteborders: bool,

    /// Quantize to the device profile's grayscale palette (dithered) and
    /// save PNG instead of full-tone JPEG.
    #[arg(long, help_heading = "Image quality")]
    pub forcepng: bool,

    /// JPEG quality (1-100). Defaults to KCC's own per-profile default: 90
    /// for Kindle Scribe/Colorsoft profiles (KS*/KCS), 85 otherwise.
    #[arg(long, value_parser = clap::value_parser!(u8).range(1..=100), help_heading = "Image quality")]
    pub jpeg_quality: Option<u8>,

    /// Keep the whole-spread copy of a double-page spread upright instead
    /// of rotating it.
    #[arg(long, help_heading = "Page layout")]
    pub norotate: bool,

    /// Put the whole-spread copy before the two halves instead of after.
    #[arg(long, help_heading = "Page layout")]
    pub rotatefirst: bool,

    /// Restack every page's two halves on top of each other (turns a 1x4
    /// strip into 2x2) instead of looking for double-page spreads.
    #[arg(long, help_heading = "Page layout")]
    pub maximizestrips: bool,

    /// Webtoon mode: join each chapter's images into one vertical strip and
    /// cut it into screen-sized pages between panels. Implies left-to-right
    /// order, white borders, no upscaling and no margin cropping.
    #[arg(short = 'w', long, help_heading = "Page layout")]
    pub webtoon: bool,

    /// Keep color pages in color instead of converting everything to
    /// grayscale. Pages with no real color are still converted.
    #[arg(long, help_heading = "Image quality")]
    pub forcecolor: bool,

    /// With --forcepng and --forcecolor, save color pages as PNG as well.
    #[arg(long = "force-png-rgb", help_heading = "Image quality")]
    pub force_png_rgb: bool,

    /// Autocontrast color pages too.
    #[arg(long, help_heading = "Image quality")]
    pub colorautocontrast: bool,

    /// With --forcepng, store pages as 8-bit grayscale instead of at the
    /// palette's own (smaller, less widely supported) bit depth.
    #[arg(long, help_heading = "Image quality")]
    pub pnglegacy: bool,

    /// With --forcepng, keep all 256 gray levels instead of quantizing to
    /// the device palette.
    #[arg(long, help_heading = "Image quality")]
    pub noquantize: bool,

    /// Leave every image exactly as it is: no cropping, resizing or
    /// recoding, whatever the profile and the other options say.
    #[arg(long, help_heading = "Conversion", display_order = 6)]
    pub noprocessing: bool,

    /// Turn pages the opposite way to the reading order. For readers that
    /// take the direction from the book; KOReader has its own setting.
    #[arg(long, help_heading = "Page layout")]
    pub invertdirection: bool,

    /// Start the book on the opposite side of a two-page (landscape) view,
    /// to line double-page spreads up. KOReader ignores it.
    #[arg(long, help_heading = "Page layout")]
    pub spreadshift: bool,

    /// Show a single centered page in a two-page (landscape) view. KOReader
    /// ignores it.
    #[arg(long, help_heading = "Page layout")]
    pub onepagelandscape: bool,

    /// For a Kobo profile's EPUB, name the file `.epub` instead of
    /// `.kepub.epub` when the name is derived from the input.
    #[arg(long, help_heading = "Conversion", display_order = 5)]
    pub nokepub: bool,

    /// Use this image as the book's cover instead of the first page. It is
    /// processed like any cover (contrast, grayscale unless --forcecolor,
    /// fitted to the device) and, in a CBZ, stored as the first image.
    /// Without this, a folder named "Covers" beside the input is looked in:
    /// an image there named like the input is its cover, or else the Nth
    /// image is the cover of the Nth book beside the input.
    #[arg(long, help_heading = "Covers and chapters")]
    pub cover: Option<PathBuf>,

    /// Join pairs of pages that are the two halves of one double-page
    /// spread into a single image before anything else is done to them.
    /// FILE is JSON as KCC's "Label Spreads" writes it — {"spreads": [12,
    /// 40]} — each number the position, counting from 0 over the whole
    /// book, of the first page of a pair. Without this, a file named like
    /// the input plus ".json" beside it is used if there is one.
    #[arg(long, value_name = "FILE", help_heading = "Covers and chapters")]
    pub spreads: Option<PathBuf>,

    /// Cut the front cover out of a wide first image (a jacket or spread
    /// scan) for the book's cover, instead of using the whole image.
    #[arg(long, help_heading = "Covers and chapters")]
    pub smartcovercrop: bool,

    /// Crop the book's cover to fill the screen instead of fitting inside it.
    #[arg(long, help_heading = "Covers and chapters")]
    pub coverfill: bool,

    /// Rotate double-page spreads clockwise instead of the default
    /// counter-clockwise.
    #[arg(long, help_heading = "Page layout")]
    pub rotateright: bool,

    /// Apply gamma correction to linearize the image. Values below 0.1 mean
    /// "use the device profile's own gamma" (always 1.0, i.e. a no-op,
    /// today).
    #[arg(short, long, help_heading = "Image quality")]
    pub gamma: Option<f32>,

    /// Set the most common dark pixel value as the black point before
    /// autocontrast.
    #[arg(long, help_heading = "Image quality")]
    pub autolevel: bool,

    /// Disable autocontrast.
    #[arg(long, help_heading = "Image quality")]
    pub noautocontrast: bool,

    /// Crop empty inter-panel sections.
    #[arg(long = "ipc", value_enum, default_value_t = InterPanelCrop::Disabled, help_heading = "Cropping and sizing")]
    pub interpanelcrop: InterPanelCrop,

    /// Erase rainbow effect on color e-ink screens by attenuating
    /// interfering frequencies.
    #[arg(long, help_heading = "Image quality")]
    pub eraserainbow: bool,

    /// Output format. `auto` picks the device family's usual one: CBZ for
    /// the four oldest Kindles, PDF for reMarkable, EPUB for everything else.
    #[arg(short, long, value_enum, default_value_t = Format::Auto, help_heading = "Conversion", display_order = 1)]
    pub format: Format,

    /// Output file or directory. Derived names follow the input, not the book title.
    /// Existing files are preserved with a numbered alternate name; the result reports the path used.
    #[arg(short, long, help_heading = "Conversion", display_order = 2)]
    pub output: Option<PathBuf>,

    /// Comic/book title (default: derived from the input file/folder name).
    #[arg(short, long, help_heading = "Book metadata")]
    pub title: Option<String>,

    /// Author name.
    #[arg(short, long, help_heading = "Book metadata")]
    pub author: Option<String>,

    /// How to use ComicInfo.xml's own Title field, if the source has one.
    #[arg(long, value_enum, default_value_t = MetadataTitle::SeriesOnly, help_heading = "Book metadata")]
    pub metadatatitle: MetadataTitle,

    /// Keep the source's ComicInfo.xml in CBZ output.
    #[arg(long, help_heading = "Book metadata")]
    pub keepcomicinfo: bool,

    /// EPUB language code.
    #[arg(long, default_value = "en-US", help_heading = "Book metadata")]
    pub language: String,

    /// Replace the screen width provided by the device profile. Can be used
    /// with `--profile OTHER` for a fully custom resolution, or alongside a
    /// named profile to override just one dimension.
    #[arg(long, help_heading = "Conversion", display_order = 3)]
    pub customwidth: Option<u32>,

    /// Replace the screen height provided by the device profile.
    #[arg(long, help_heading = "Conversion", display_order = 4)]
    pub customheight: Option<u32>,

    /// Expect the input's chapter folders to be nested one level deeper
    /// under a volume folder (Volume/Chapter/pages - what Mangabind's
    /// `-combine` mode produces) and build a two-level table of contents:
    /// a volume entry, its chapters nested underneath. EPUB output only for
    /// now; combining this with --format cbz or --format pdf is refused.
    #[arg(long, help_heading = "Covers and chapters")]
    pub nested_toc: bool,
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
    Auto,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{error::ErrorKind, CommandFactory};

    #[test]
    fn help_grouping_uses_claps_argument_metadata() {
        let mut command = Cli::command();
        command.clone().debug_assert();
        command.build();
        for arg in command.get_arguments().filter(|arg| !arg.is_positional()) {
            if !matches!(arg.get_id().as_str(), "help" | "version") {
                assert!(
                    arg.get_help_heading().is_some(),
                    "{} needs a help heading",
                    arg.get_id()
                );
            }
        }
    }

    #[test]
    fn usage_modes_and_documented_examples_preserve_parsing_and_defaults() {
        for arguments in [
            vec!["mangapress"],
            vec!["mangapress", "--quiet"],
            vec!["mangapress", "--dry-run"],
        ] {
            assert_eq!(
                Cli::try_parse_from(arguments).unwrap_err().kind(),
                ErrorKind::MissingRequiredArgument
            );
        }
        assert!(
            Cli::try_parse_from(["mangapress", "--list-profiles"])
                .unwrap()
                .list_profiles
        );
        assert!(
            Cli::try_parse_from(["mangapress", "--protocol-version"])
                .unwrap()
                .protocol_version
        );

        let defaults = Cli::try_parse_from(["mangapress", "Volume 1.cbz"]).unwrap();
        assert_eq!(defaults.profile, "KV");
        assert_eq!(defaults.format, Format::Auto);
        assert_eq!(defaults.cropping, Cropping::MarginsAndPageNumbers);
        assert!(defaults.jpeg_quality.is_none());

        let conversion = Cli::try_parse_from([
            "mangapress",
            "Volume 1.cbz",
            "--profile",
            "K11",
            "--format",
            "epub",
        ])
        .unwrap();
        assert_eq!(conversion.profile, "K11");
        assert_eq!(conversion.format, Format::Epub);
        let preview = Cli::try_parse_from([
            "mangapress",
            "Volume 1.cbz",
            "--profile",
            "K11",
            "--dry-run",
        ])
        .unwrap();
        assert!(preview.dry_run);
    }
}
