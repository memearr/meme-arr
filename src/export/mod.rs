//! Turn a chosen clip window into an animated WebP / MP4 / WebM / GIF, with
//! optional burned source subtitles or custom meme text.
//!
//! Filtergraph construction is pure and unit-tested; running ffmpeg and writing
//! temp caption files happens in [`run_export`].

use crate::ffmpeg;
use crate::subtitles::{self, Cue};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::warn;

mod filters;
mod request;
mod run;
#[cfg(test)]
mod tests;

pub use self::filters::*;
pub use self::request::*;
pub use self::run::*;

/// `(srt_path, top_text_path, bottom_text_path, files_to_clean_up)`.
type CaptionFilePaths = (Option<String>, Option<String>, Option<String>, CaptionFiles);
