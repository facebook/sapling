//! Support for `InterfaceMode::Direct` and other modes using `Direct`.

use std::collections::HashSet;
use std::time::Duration;
use std::time::Instant;

use termwiz::input::InputEvent;
use termwiz::surface::CursorVisibility;
use termwiz::surface::Position;
use termwiz::surface::change::Change;
use termwiz::terminal::Terminal;
use vec_map::VecMap;

use crate::action::Action;
use crate::config::InterfaceMode;
use crate::config::WrappingMode;
use crate::error::Error;
use crate::error::Result;
use crate::event::Event;
use crate::event::EventStream;
use crate::file::File;
use crate::file::FileInfo;
use crate::line::Line;
use crate::progress::Progress;

/// Return value of `direct`.
#[derive(Debug)]
pub(crate) enum Outcome {
    /// Content is not completely rendered.  A hint to enter full-screen.
    /// The number of rows that have been rendered is included.
    RenderIncomplete(usize),

    /// Content is not rendered at all.  A hint to enter full-screen.
    RenderNothing,

    /// Content is completely rendered.
    RenderComplete,

    /// The user pressed a key to exit.
    Interrupted,
}

/// Streaming content to the terminal without entering full screen.
///
/// Similar to `tail -f`, but with dynamic progress support.
/// Useful for rendering content before entering the full-screen mode.
///
/// Lines are rendered in this order:
/// - Output (append-only)
/// - Error (append-only)
/// - Progress (mutable)
///
/// Return `Outcome::Interrupted` if `q` or `Ctrl+C` is pressed.
/// Otherwise, return values and conditions are as follows:
///
/// | Interface  | Fits Screen | Streams Ended | Return           |
/// |------------|-------------|---------------|------------------|
/// | FullScreen | (any)       | (any)         | RenderNothing    |
/// | Direct     | (any)       | no            | -                |
/// | Direct     | (any)       | yes           | RenderComplete   |
/// | Hybrid     | yes         | no            | -                |
/// | Hybrid     | yes         | yes           | RenderComplete   |
/// | Hybrid     | no          | (any)         | RenderIncomplete |
/// | Delayed    | (any)       | no (time out) | RenderNothing    |
/// | Delayed    | yes         | yes           | RenderComplete   |
/// | Delayed    | no          | yes           | RenderNothing    |
pub(crate) fn direct<T: Terminal>(
    term: &mut T,
    output_files: &[File],
    error_files: &[File],
    progress: Option<&Progress>,
    events: &mut EventStream,
    mode: InterfaceMode,
    poll_input: bool,
) -> Result<Outcome> {
    if mode == InterfaceMode::FullScreen {
        return Ok(Outcome::RenderNothing);
    }
    let delayed_deadline = match mode {
        InterfaceMode::Delayed(duration) => Some(Instant::now() + duration),
        _ => None,
    };
    let mut loading = HashSet::with_capacity(output_files.len() + error_files.len());
    for file in output_files.iter().chain(error_files.iter()) {
        loading.insert(file.index());
    }

    let mut last_read = VecMap::new(); // file index -> line number last read
    let mut collect_unread = |files: &[File], max_lines: usize| -> Vec<Vec<u8>> {
        let mut result = Vec::new();
        for file in files.iter() {
            let index = file.index();
            let mut lines = file.lines();
            let last = last_read.get(index).cloned().unwrap_or(0);
            file.set_needed_lines(last + max_lines);
            // Ignore the incomplete last line if the file is loading.
            if lines > 0
                && !file.loaded()
                && file
                    .with_line(lines - 1, |l| !l.ends_with(b"\n"))
                    .unwrap_or(true)
            {
                lines -= 1;
            }
            if lines >= last {
                let lines = (last + max_lines).min(lines);
                result.reserve(lines - last);
                for i in last..lines {
                    file.with_line(i, |l| result.push(l.to_vec()));
                }
                last_read.insert(index, lines);
            }
        }
        result
    };

    let read_progress_lines = || -> Vec<Vec<u8>> {
        let line_count = progress.map_or(0, |p| p.lines());
        (0..line_count)
            .filter_map(|i| progress.and_then(|p| p.with_line(i, |l| l.to_vec())))
            .collect::<Vec<_>>()
    };

    let mut state = StreamingLines::default();
    let delayed = delayed_deadline.is_some();
    let has_one_screen_limit = !matches!(mode, InterfaceMode::Direct);
    let mut render = |term: &mut T, h: usize, w: usize| -> Result<Option<Outcome>> {
        let append_output_lines = collect_unread(output_files, h + 2);
        let append_error_lines = collect_unread(error_files, h + 2);
        let progress_lines = read_progress_lines();
        state.add_lines(append_output_lines, append_error_lines, progress_lines);
        if delayed {
            if has_one_screen_limit && state.height(w) >= h {
                return Ok(Some(Outcome::RenderNothing));
            }
        } else {
            if has_one_screen_limit && state.height(w) >= h {
                return Ok(Some(Outcome::RenderIncomplete(state.rendered_row_count())));
            }
            let changes = state.render_pending_lines(w)?;
            term.render(&changes).map_err(Error::Termwiz)?;
        }
        Ok(None)
    };

    let mut size = term.get_screen_size().map_err(Error::Termwiz)?;
    let mut loaded = HashSet::with_capacity(loading.capacity());
    let mut remaining = output_files.len() + error_files.len();
    let interval = Duration::from_millis(10);
    while remaining > 0 {
        let maybe_event = if poll_input {
            events.get(term, Some(interval))?
        } else {
            events.try_recv(Some(interval))?
        };
        match maybe_event {
            Some(Event::Loaded(i)) => {
                if loading.contains(&i) && loaded.insert(i) {
                    remaining -= 1;
                }
            }
            Some(Event::Input(InputEvent::Resized { .. })) => {
                size = term.get_screen_size().map_err(Error::Termwiz)?;
            }
            Some(Event::Input(InputEvent::Key(key))) => {
                use termwiz::input::KeyCode::Char;
                use termwiz::input::Modifiers;
                match (key.modifiers, key.key) {
                    (Modifiers::NONE, Char('q')) | (Modifiers::CTRL, Char('c')) => {
                        term.render(&state.abort()).map_err(Error::Termwiz)?;
                        return Ok(Outcome::Interrupted);
                    }
                    (Modifiers::NONE, Char('f')) | (Modifiers::NONE, Char(' ')) => {
                        let outcome = if delayed {
                            Outcome::RenderNothing
                        } else {
                            Outcome::RenderIncomplete(state.rendered_row_count())
                        };
                        return Ok(outcome);
                    }
                    _ => (),
                }
            }
            Some(Event::Action(Action::Quit)) => {
                term.render(&state.abort()).map_err(Error::Termwiz)?;
                return Ok(Outcome::Interrupted);
            }
            _ => (),
        }
        if let Some(deadline) = delayed_deadline {
            if deadline <= Instant::now() {
                return Ok(Outcome::RenderNothing);
            }
        }
        if let Some(outcome) = render(term, size.rows, size.cols)? {
            return Ok(outcome);
        }
    }

    if delayed {
        term.render(&state.render_pending_lines(size.cols)?)
            .map_err(Error::Termwiz)?;
    }

    Ok(Outcome::RenderComplete)
}

/// State for calculating how to incrementally render streaming changes.
///
/// +----------------------------+
/// | past output (never redraw) |
/// +----------------------------+
/// | new output (just received) |
/// +----------------------------+
/// | error (always redraw)      |
/// +----------------------------+
/// | progress (always redraw)   |
/// +----------------------------+
#[derive(Default)]
struct StreamingLines {
    past_output_row_count: usize,
    new_output_lines: Vec<Vec<u8>>,
    error_lines: Vec<Vec<u8>>,
    progress_lines: Vec<Vec<u8>>,
    erase_row_count: usize,
    pending_changes: bool,
    cursor_hidden: bool,
}

impl StreamingLines {
    fn add_lines(
        &mut self,
        mut append_output_lines: Vec<Vec<u8>>,
        mut append_error_lines: Vec<Vec<u8>>,
        replace_progress_lines: Vec<Vec<u8>>,
    ) {
        if append_output_lines.is_empty()
            && append_error_lines.is_empty()
            && replace_progress_lines == self.progress_lines
        {
            return;
        }
        self.new_output_lines.append(&mut append_output_lines);
        self.error_lines.append(&mut append_error_lines);
        self.progress_lines = replace_progress_lines;
        self.pending_changes = true;
    }

    fn render_pending_lines(&mut self, terminal_width: usize) -> Result<Vec<Change>> {
        // Fast path: nothing changed?
        if !self.pending_changes {
            return Ok(Vec::new());
        }

        // Every line needs at least 2 `Change`s: Text, and CursorPosition,
        // plus 2 Changes for erasing existing lines.
        let line_count =
            self.new_output_lines.len() + self.error_lines.len() + self.progress_lines.len();
        let mut changes = Vec::with_capacity(line_count * 2 + 2);

        // Step 1: Erase progress, and error.
        if self.erase_row_count > 0 {
            let dy = -(self.erase_row_count as isize);
            changes.push(Change::CursorPosition {
                x: Position::Relative(0),
                y: Position::Relative(dy),
            });
            changes.push(Change::ClearToEndOfScreen(Default::default()));
        }

        // Step 2: Render new output + error + progress
        let mut render = |lines| -> Result<_> {
            let mut row_count = 0;
            for line in lines {
                let line = Line::new(0, line);
                let height = line.height(terminal_width, WrappingMode::GraphemeBoundary);
                line.render(&mut changes, 0, terminal_width * height, None);
                changes.push(Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Relative(1),
                });
                row_count += height;
            }
            Ok(row_count)
        };

        let new_output_row_count = render(self.new_output_lines.iter())?;
        let error_row_count = render(self.error_lines.iter())?;
        let mut progress_row_count = render(self.progress_lines.iter())?;

        // Don't render the last newline after progress, and hide the
        // cursor while progress is being shown.
        if progress_row_count > 0 {
            changes.pop();
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(0),
            });
            if !self.cursor_hidden {
                changes.push(Change::CursorVisibility(CursorVisibility::Hidden));
                self.cursor_hidden = true;
            }
            progress_row_count -= 1;
        } else if self.cursor_hidden {
            changes.push(Change::CursorVisibility(CursorVisibility::Visible));
            self.cursor_hidden = false;
        }

        // Step 3: Update internal state.
        self.past_output_row_count += new_output_row_count;
        self.new_output_lines.clear();
        self.erase_row_count = error_row_count + progress_row_count;
        self.pending_changes = false;

        Ok(changes)
    }

    fn abort(&mut self) -> Vec<Change> {
        let mut changes = Vec::new();
        if self.cursor_hidden {
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Relative(1),
            });
            changes.push(Change::CursorVisibility(CursorVisibility::Visible));
            self.cursor_hidden = false;
        }
        changes
    }

    fn height(&self, terminal_width: usize) -> usize {
        let mut row_count = self.past_output_row_count;
        for line in self
            .new_output_lines
            .iter()
            .chain(self.error_lines.iter())
            .chain(self.progress_lines.iter())
        {
            let line = Line::new(0, line);
            row_count += line.height(terminal_width, WrappingMode::GraphemeBoundary);
        }
        row_count
    }

    fn rendered_row_count(&self) -> usize {
        self.past_output_row_count + self.erase_row_count
    }
}
