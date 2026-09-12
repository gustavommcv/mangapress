mod args;

use anyhow::{bail, Context};
use args::Cli;
use clap::Parser;
use mangapress_core::profile::Profile;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let profile = Profile::by_code(&cli.profile)
        .with_context(|| format!("unknown device profile '{}'", cli.profile))?;

    if !cli.input.exists() {
        bail!("input path does not exist: {}", cli.input.display());
    }

    let (width, height) = profile.effective_resolution(cli.customwidth, cli.customheight);
    if width == 0 || height == 0 {
        bail!(
            "resolved target resolution is {}x{} — profile '{}' has no built-in resolution, \
             pass both --customwidth and --customheight to set one",
            width,
            height,
            cli.profile
        );
    }

    println!(
        "mangapress: would convert '{}' for {} ({}x{}, {} gray levels), manga_style={}, format={:?}",
        cli.input.display(),
        profile.display_name,
        width,
        height,
        profile.palette.levels(),
        cli.manga_style,
        cli.format,
    );

    bail!(
        "conversion pipeline is not implemented yet — this is a scaffold build. \
         See docs/adr/ and the module docs in mangapress-core for the implementation plan."
    );
}
