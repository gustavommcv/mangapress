//! Right-to-left ("manga style") reading order.
//!
//! Contrary to a natural first guess, KCC does **not** reorder the book's
//! page/file sequence for manga mode — `-m/--manga-style` only flips
//! `righttoleft`, whose effects are entirely local, in five places
//! (see `docs/adr/0005-mangabind-contract.md` for how this interacts with
//! chapter ordering specifically):
//!
//! 1. EPUB OPF spine `page-progression-direction` (`rtl` vs `ltr`) and the
//!    `primary-writing-mode` CSS metadata — this is what actually makes
//!    readers paginate right-to-left. See [`crate::ebook::epub`].
//! 2. Which half of a split double-page spread becomes "page one" vs "page
//!    two" — see [`crate::pipeline::spread`].
//! 3. Kindle Panel View navigation quadrant order (low priority — desktop/
//!    Kindle-firmware-specific feature, not core to the EPUB/CBZ/PDF scope).
//! 4. Smart-cover-crop: which half of a wide cover image is kept.
//! 5. The manual spread-merge JSON sidecar's left/right paste order.
//!
//! `--webtoon` forces `righttoleft = false` unconditionally.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadingDirection {
    pub right_to_left: bool,
}

impl ReadingDirection {
    pub fn epub_page_progression(self) -> &'static str {
        if self.right_to_left {
            "rtl"
        } else {
            "ltr"
        }
    }
}
