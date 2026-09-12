//! Device profiles: target resolution, grayscale palette depth, and gamma
//! per e-reader model.
//!
//! Transcribed from `kindlecomicconverter/image.py`'s `ProfileData` class
//! (KCC upstream, commit `ea532c709b72a994fd9219c3bb7cd3f1df08027b`), which
//! is the closest thing to a spec for "what resolution does device X want."
//! Every profile in upstream ships gamma `1.0`, so gamma correction is only
//! ever a no-op unless the user explicitly overrides it (`-g/--gamma`).

/// Grayscale palette depth used for palette-quantized (PNG) output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Palette {
    /// 4 gray levels (Kindle 1 era).
    Gray4,
    /// 15 gray levels (Kindle 2 era).
    Gray15,
    /// 16 gray levels (every modern Kindle/Kobo/reMarkable).
    Gray16,
}

impl Palette {
    /// Number of distinct gray levels in the palette.
    pub fn levels(self) -> u8 {
        match self {
            Palette::Gray4 => 4,
            Palette::Gray15 => 15,
            Palette::Gray16 => 16,
        }
    }
}

/// A named device target: display name, screen resolution, palette, and gamma.
#[derive(Debug, Clone, Copy)]
pub struct Profile {
    pub code: &'static str,
    pub display_name: &'static str,
    pub width: u32,
    pub height: u32,
    pub palette: Palette,
    pub gamma: f32,
}

/// Device family, used for format defaults and Kindle/Kobo-specific
/// behavior (Panel View is Kindle-only, KEPUB naming is Kobo-only, etc. —
/// see `docs/adr/0005-mangabind-contract.md` and the pipeline module for
/// where these distinctions actually apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Kindle,
    Kobo,
    Remarkable,
    Other,
}

impl Profile {
    pub fn family(&self) -> Family {
        if self.code == "OTHER" {
            Family::Other
        } else if self.code.starts_with("Rmk") {
            Family::Remarkable
        } else if self.code.starts_with("Ko") {
            Family::Kobo
        } else {
            Family::Kindle
        }
    }

    /// Look up a profile by its short CLI code (e.g. `"KV"`, `"KoAO"`).
    pub fn by_code(code: &str) -> Option<&'static Profile> {
        PROFILES.iter().find(|p| p.code == code)
    }

    /// Resolution to actually target, after applying KCC's `--customwidth`/
    /// `--customheight` overrides (`comic2ebook.py`'s `checkOptions()`):
    /// each dimension is independently replaceable, so `OTHER` + one
    /// override + the other left at `0` is a real (if probably unintended)
    /// combination the caller should validate, not something this function
    /// rejects on its own.
    pub fn effective_resolution(
        &self,
        width_override: Option<u32>,
        height_override: Option<u32>,
    ) -> (u32, u32) {
        (
            width_override.unwrap_or(self.width),
            height_override.unwrap_or(self.height),
        )
    }
}

macro_rules! profile {
    ($code:literal, $name:literal, $w:literal, $h:literal, $pal:expr) => {
        Profile {
            code: $code,
            display_name: $name,
            width: $w,
            height: $h,
            palette: $pal,
            gamma: 1.0,
        }
    };
}

/// All built-in device profiles, in upstream KCC's declaration order.
pub static PROFILES: &[Profile] = &[
    // --- Kindle (ProfilesKindlePDOC) ---
    profile!("K1", "Kindle 1", 600, 670, Palette::Gray4),
    profile!("K2", "Kindle 2", 600, 670, Palette::Gray15),
    profile!("KDX", "Kindle DX/DXG", 824, 1000, Palette::Gray16),
    profile!("K34", "Kindle Keyboard/Touch", 600, 800, Palette::Gray16),
    profile!("K57", "Kindle 5/7", 600, 800, Palette::Gray16),
    profile!("KPW", "Kindle Paperwhite 1/2", 758, 1024, Palette::Gray16),
    profile!("KV", "Kindle Voyage", 1072, 1448, Palette::Gray16),
    profile!(
        "KPW34",
        "Kindle Paperwhite 3/4/Oasis",
        1072,
        1448,
        Palette::Gray16
    ),
    profile!("K810", "Kindle 8/10", 600, 800, Palette::Gray16),
    profile!("KO", "Kindle Oasis 2/3", 1264, 1680, Palette::Gray16),
    profile!("K11", "Kindle 11", 1072, 1448, Palette::Gray16),
    profile!(
        "KPW5",
        "Kindle Paperwhite 5/Signature Edition",
        1236,
        1648,
        Palette::Gray16
    ),
    profile!("KPW6", "Kindle Paperwhite 6", 1272, 1696, Palette::Gray16),
    profile!("KS1860", "Kindle 1860", 1860, 1920, Palette::Gray16),
    profile!("KS1920", "Kindle 1920", 1920, 1920, Palette::Gray16),
    profile!("KS1240", "Kindle 1240", 1240, 1860, Palette::Gray16),
    profile!("KS1324", "Kindle 1324", 1324, 1986, Palette::Gray16),
    profile!("KS", "Kindle Scribe 1/2", 1860, 2480, Palette::Gray16),
    profile!("KCS", "Kindle Colorsoft", 1272, 1696, Palette::Gray16),
    profile!("KS3", "Kindle Scribe 3", 1986, 2648, Palette::Gray16),
    profile!(
        "KSCS",
        "Kindle Scribe Colorsoft",
        1986,
        2648,
        Palette::Gray16
    ),
    // --- Kobo (ProfilesKobo) ---
    profile!("KoMT", "Kobo Mini/Touch", 600, 800, Palette::Gray16),
    profile!("KoG", "Kobo Glo", 768, 1024, Palette::Gray16),
    profile!("KoGHD", "Kobo Glo HD", 1072, 1448, Palette::Gray16),
    profile!("KoA", "Kobo Aura", 758, 1024, Palette::Gray16),
    profile!("KoAHD", "Kobo Aura HD", 1080, 1440, Palette::Gray16),
    profile!("KoAH2O", "Kobo Aura H2O", 1080, 1430, Palette::Gray16),
    profile!("KoAO", "Kobo Aura ONE", 1404, 1872, Palette::Gray16),
    profile!("KoN", "Kobo Nia", 758, 1024, Palette::Gray16),
    profile!(
        "KoC",
        "Kobo Clara HD/Kobo Clara 2E",
        1072,
        1448,
        Palette::Gray16
    ),
    profile!("KoCC", "Kobo Clara Colour", 1072, 1448, Palette::Gray16),
    profile!(
        "KoL",
        "Kobo Libra H2O/Kobo Libra 2",
        1264,
        1680,
        Palette::Gray16
    ),
    profile!("KoLC", "Kobo Libra Colour", 1264, 1680, Palette::Gray16),
    profile!("KoF", "Kobo Forma", 1440, 1920, Palette::Gray16),
    profile!("KoS", "Kobo Sage", 1440, 1920, Palette::Gray16),
    profile!("KoE", "Kobo Elipsa", 1404, 1872, Palette::Gray16),
    // --- reMarkable (ProfilesRemarkable) ---
    profile!("Rmk1", "reMarkable 1", 1404, 1872, Palette::Gray16),
    profile!("Rmk2", "reMarkable 2", 1404, 1872, Palette::Gray16),
    profile!("RmkPP", "reMarkable Paper Pro", 1620, 2160, Palette::Gray16),
    profile!(
        "RmkPPMove",
        "reMarkable Paper Pro Move",
        954,
        1696,
        Palette::Gray16
    ),
    // --- Generic fallback ---
    profile!("OTHER", "Other", 0, 0, Palette::Gray16),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_up_known_profile() {
        let p = Profile::by_code("KV").expect("KV must exist");
        assert_eq!(p.width, 1072);
        assert_eq!(p.height, 1448);
        assert_eq!(p.family(), Family::Kindle);
    }

    #[test]
    fn classifies_families_by_code_prefix() {
        assert_eq!(Profile::by_code("KoAO").unwrap().family(), Family::Kobo);
        assert_eq!(
            Profile::by_code("Rmk1").unwrap().family(),
            Family::Remarkable
        );
        assert_eq!(Profile::by_code("OTHER").unwrap().family(), Family::Other);
    }

    #[test]
    fn unknown_code_is_none() {
        assert!(Profile::by_code("NOPE").is_none());
    }

    #[test]
    fn custom_resolution_overrides_one_or_both_dimensions() {
        let p = Profile::by_code("KV").unwrap();
        assert_eq!(p.effective_resolution(None, None), (1072, 1448));
        assert_eq!(p.effective_resolution(Some(1200), None), (1200, 1448));
        assert_eq!(p.effective_resolution(None, Some(1600)), (1072, 1600));
        assert_eq!(p.effective_resolution(Some(1200), Some(1600)), (1200, 1600));
    }

    #[test]
    fn every_profile_code_is_unique() {
        let mut codes: Vec<&str> = PROFILES.iter().map(|p| p.code).collect();
        codes.sort_unstable();
        let mut deduped = codes.clone();
        deduped.dedup();
        assert_eq!(codes.len(), deduped.len(), "duplicate profile code found");
    }
}
