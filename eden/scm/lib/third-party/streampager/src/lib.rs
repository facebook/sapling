//! Stream Pager
//!
//! A pager for streams.
#![warn(missing_docs)]
#![recursion_limit = "1024"]
#![allow(clippy::comparison_chain)]

pub mod action;
mod bar;
pub mod bindings;
mod buffer;
#[cfg(feature = "load_file")]
mod buffer_cache;
mod command;
pub mod config;
pub mod control;
mod direct;
mod display;
pub mod error;
mod event;
pub mod file;
mod help;
mod keymap_error;
#[cfg(feature = "keymap-file")]
mod keymap_file;
#[macro_use]
mod keymap_macro;
mod keymaps;
mod line;
mod line_cache;
mod line_drawing;
mod loaded_file;
mod overstrike;
pub mod pager;
mod progress;
mod prompt;
mod prompt_history;
mod refresh;
mod ruler;
mod screen;
mod search;
mod util;
pub(crate) mod spanset;

pub use error::{Error, Result};
pub use file::FileIndex;
pub use pager::Pager;
