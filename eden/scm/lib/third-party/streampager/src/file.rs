//! Files.

use std::borrow::Cow;

use enum_dispatch::enum_dispatch;

pub(crate) use crate::control::ControlledFile;
pub(crate) use crate::loaded_file::LoadedFile;

/// An identifier for a file streampager is paging.
pub type FileIndex = usize;

/// Default value for `needed_lines`.
pub(crate) const DEFAULT_NEEDED_LINES: usize = 5000;

/// Trait for getting information from a file.
#[enum_dispatch]
pub(crate) trait FileInfo {
    /// The file's index.
    fn index(&self) -> FileIndex;

    /// The file's title.
    fn title(&self) -> Cow<'_, str>;

    /// The file's info.
    fn info(&self) -> Cow<'_, str>;

    /// True once the file is loaded and all newlines have been parsed.
    fn loaded(&self) -> bool;

    /// Returns the number of lines in the file.
    fn lines(&self) -> usize;

    /// Runs the `call` function, passing it the contents of line `index`.
    /// Tries to avoid copying the data if possible, however the borrowed
    /// line only lasts as long as the function call.
    fn with_line<T, F>(&self, index: usize, call: F) -> Option<T>
    where
        F: FnMut(Cow<'_, [u8]>) -> T;

    /// Set how many lines are needed.
    ///
    /// If `self.lines()` exceeds that number, pause loading until
    /// `set_needed_lines` is called with a larger number.
    /// This is only effective for "streamed" input.
    fn set_needed_lines(&self, lines: usize);

    /// True if the loading thread has been paused.
    fn paused(&self) -> bool;
}

/// A file.
#[enum_dispatch(FileInfo)]
#[derive(Clone)]
pub(crate) enum File {
    LoadedFile,
    ControlledFile,
}
