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

mod ass;
mod caption;
mod filters;
mod grade;
mod layers;
mod music;
mod path;
mod reframe;
mod request;
mod run;
mod still;
#[cfg(test)]
mod tests;
mod transition;

pub use self::ass::*;
pub use self::caption::*;
pub use self::filters::*;
pub use self::grade::*;
pub use self::layers::*;
pub use self::music::*;
pub use self::path::*;
pub use self::reframe::*;
pub use self::request::*;
pub use self::run::*;
pub use self::still::*;
pub use self::transition::*;
