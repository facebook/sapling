//! Controlled files.
//!
//! Files where data is provided by a controller.

use std::borrow::Cow;
use std::ops::Range;
use std::sync::{Arc, Mutex, RwLock};

use thiserror::Error;

use crate::event::{Event, EventSender};
use crate::file::{FileIndex, FileInfo};

/// Errors that may occur during controlled file operations.
#[derive(Debug, Error)]
pub enum ControlledFileError {
    /// Line number out of range.
    #[error("line number {index} out of range (0..{length})")]
    LineOutOfRange {
        /// The index of the line number that is out of range.
        index: usize,
        /// The length of the file (and so the limit for the line number).
        length: usize,
    },

    /// Other error type.
    #[error(transparent)]
    Error(#[from] crate::error::Error),
}

/// Result alias for controlled file operations that may fail.
pub type Result<T> = std::result::Result<T, ControlledFileError>;

/// A controller for a controlled file.
///
/// This contains a logical file which can be mutated by a controlling
/// program.  It can be added to the pager using
/// `Pager::add_controlled_file`.
#[derive(Clone)]
pub struct Controller {
    data: Arc<RwLock<FileData>>,
    notify: Arc<Mutex<Vec<(EventSender, FileIndex)>>>,
}

impl Controller {
    /// Create a new controller.  The controlled file is initially empty.
    pub fn new(title: impl Into<String>) -> Controller {
        Controller {
            data: Arc::new(RwLock::new(FileData::new(title))),
            notify: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Returns a copy of the current title.
    pub fn title(&self) -> String {
        let data = self.data.read().unwrap();
        data.title.clone()
    }

    /// Returns a copy of the current file info.
    pub fn info(&self) -> String {
        let data = self.data.read().unwrap();
        data.info.clone()
    }

    /// Apply a sequence of changes to the controlled file.
    pub fn apply_changes(&self, changes: impl IntoIterator<Item = Change>) -> Result<()> {
        let mut data = self.data.write().unwrap();
        for change in changes {
            data.apply_change(change)?;
        }
        // TODO(markbt): more fine-grained notifications.
        // For now, just reload the file.
        let notify = self.notify.lock().unwrap();
        for (event_sender, index) in notify.iter() {
            event_sender.send(Event::Reloading(*index))?;
        }
        Ok(())
    }
}

/// A change to apply to a controlled file.
pub enum Change {
    /// Set the title for the file.
    SetTitle {
        /// The new title.
        title: String,
    },

    /// Set the file information for the file.
    SetInfo {
        /// The text of the new file info.
        info: String,
    },

    /// Append a single line to the file.
    AppendLine {
        /// The content of the new line.
        content: Vec<u8>,
    },

    /// Insert a single line into the file.
    InsertLine {
        /// Index of the line in the file to insert before.
        before_index: usize,
        /// The content of the new line.
        content: Vec<u8>,
    },

    /// Replace a single line in the file.
    ReplaceLine {
        /// Index of the line in fhe file to replace.
        index: usize,
        /// The content of the new line.
        content: Vec<u8>,
    },

    /// Delete a single line from the file.
    DeleteLine {
        /// Index of the line in the file to delete.
        index: usize,
    },

    /// Append multiple lines to the file
    AppendLines {
        /// The contents of the new lines.
        contents: Vec<Vec<u8>>,
    },

    /// Insert some lines before another line in the file.
    InsertLines {
        /// Index of the line in the file to insert before.
        before_index: usize,
        /// The contents of the new lines.
        contents: Vec<Vec<u8>>,
    },

    /// Replace a range of lines with another set of lines.
    /// The range and the new lines do not need to be the same size.
    ReplaceLines {
        /// The range of lines in the file to replace.
        range: Range<usize>,
        /// The contents of the new lines.
        contents: Vec<Vec<u8>>,
    },

    /// Delete a range of lines in the file.
    DeleteLines {
        /// The range of lines in the file to delete.
        range: Range<usize>,
    },

    /// Replace all lines with another set of lines.
    ReplaceAll {
        /// The new contents of the file.
        contents: Vec<Vec<u8>>,
    },
}

/// A file whose contents is controlled by a `Controller`.
#[derive(Clone)]
pub struct ControlledFile {
    index: FileIndex,
    data: Arc<RwLock<FileData>>,
}

impl ControlledFile {
    pub(crate) fn new(
        controller: &Controller,
        index: FileIndex,
        event_sender: EventSender,
    ) -> ControlledFile {
        let mut notify = controller.notify.lock().unwrap();
        notify.push((event_sender, index));
        ControlledFile {
            index,
            data: controller.data.clone(),
        }
    }
}

impl FileInfo for ControlledFile {
    /// The file's index.
    fn index(&self) -> FileIndex {
        self.index
    }

    /// The file's title.
    fn title(&self) -> Cow<'_, str> {
        let data = self.data.read().unwrap();
        Cow::Owned(data.title.clone())
    }

    /// The file's info.
    fn info(&self) -> Cow<'_, str> {
        let data = self.data.read().unwrap();
        Cow::Owned(data.info.clone())
    }

    /// True once the file is loaded and all newlines have been parsed.
    fn loaded(&self) -> bool {
        true
    }

    /// Returns the number of lines in the file.
    fn lines(&self) -> usize {
        self.data.read().unwrap().lines.len()
    }

    /// Runs the `call` function, passing it the contents of line `index`.
    /// Tries to avoid copying the data if possible, however the borrowed
    /// line only lasts as long as the function call.
    fn with_line<T, F>(&self, index: usize, mut call: F) -> Option<T>
    where
        F: FnMut(Cow<'_, [u8]>) -> T,
    {
        let data = self.data.read().unwrap();
        data.lines.get(index).map(|line| call(Cow::Borrowed(line.content.as_slice())))
    }

    /// Set how many lines are needed.
    ///
    /// If `self.lines()` exceeds that number, pause loading until
    /// `set_needed_lines` is called with a larger number.
    /// This is only effective for "streamed" input.
    fn set_needed_lines(&self, _lines: usize) {}

    /// True if the loading thread has been paused.
    fn paused(&self) -> bool {
        false
    }
}

struct FileData {
    title: String,
    info: String,
    lines: Vec<LineData>,
}

impl FileData {
    fn new(title: impl Into<String>) -> FileData {
        FileData {
            title: title.into(),
            info: String::new(),
            lines: Vec::new(),
        }
    }

    fn line_mut(&mut self, index: usize) -> Result<&mut LineData> {
        let length = self.lines.len();
        if let Some(line) = self.lines.get_mut(index) {
            return Ok(line);
        }
        Err(ControlledFileError::LineOutOfRange { index, length })
    }

    fn apply_change(&mut self, change: Change) -> Result<()> {
        match change {
            Change::SetTitle { title } => {
                self.title = title;
            }
            Change::SetInfo { info } => {
                self.info = info;
            }
            Change::AppendLine { content } => {
                self.lines.push(LineData::with_content(content));
            }
            Change::InsertLine {
                before_index,
                content,
            } => {
                self.lines
                    .insert(before_index, LineData::with_content(content));
            }
            Change::ReplaceLine { index, content } => {
                self.line_mut(index)?.content = content;
            }
            Change::DeleteLine { index } => {
                self.lines.remove(index);
            }
            Change::AppendLines { contents } => {
                let new_lines = contents.into_iter().map(LineData::with_content);
                self.lines.extend(new_lines);
            }
            Change::InsertLines {
                before_index,
                contents,
            } => {
                let new_lines = contents.into_iter().map(LineData::with_content);
                self.lines.splice(before_index..before_index, new_lines);
            }
            Change::ReplaceLines { range, contents } => {
                let new_lines = contents.into_iter().map(LineData::with_content);
                self.lines.splice(range, new_lines);
            }
            Change::DeleteLines { range } => {
                self.lines.splice(range, std::iter::empty());
            }
            Change::ReplaceAll { contents } => {
                let new_lines = contents.into_iter().map(LineData::with_content);
                self.lines = new_lines.collect();
            }
        }
        Ok(())
    }
}

struct LineData {
    content: Vec<u8>,
}

impl LineData {
    fn with_content(content: Vec<u8>) -> LineData {
        LineData { content }
    }
}
