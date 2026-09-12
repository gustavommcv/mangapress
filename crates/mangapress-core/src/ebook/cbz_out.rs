//! Resized-`.cbz` output: just the processed pages re-zipped, optionally
//! carrying forward `ComicInfo.xml` (`--keepcomicinfo`).

use super::Chapter;
use crate::error::Result;

pub fn build_cbz(_chapters: &[Chapter]) -> Result<Vec<u8>> {
    todo!("flatten chapters back into a single zip via archive::cbz::write_zip")
}
