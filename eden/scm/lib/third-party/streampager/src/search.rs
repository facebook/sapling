//! Searching.

use std::borrow::Cow;
use std::cmp::min;
use std::ops::RangeInclusive;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time;

use regex::bytes::{NoExpand, Regex};
use termwiz::cell::CellAttributes;
use termwiz::color::AnsiColor;
use termwiz::surface::Position;
use termwiz::surface::change::Change;
use unicode_width::UnicodeWidthStr;

use crate::error::Error;
use crate::event::{Event, EventSender};
use crate::file::{File, FileInfo};
use crate::overstrike;
use crate::spanset::SpanSet;

const SEARCH_BATCH_SIZE: usize = 10000;

/// Regex for detecting and removing escape sequences during search.
pub(crate) static ESCAPE_SEQUENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("\x1B\\[[0123456789:;\\[?!\"'#%()*+ ]{0,32}m").unwrap());

/// What kind of search to perform.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SearchKind {
    First,
    FirstAfter(usize),
    FirstBefore(usize),
}

/// Motion when changing search matches.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum MatchMotion {
    First,
    Previous,
    PreviousLine,
    PreviousScreen,
    Next,
    NextLine,
    NextScreen,
    Last,
}

/// Internal struct for searching in a file.  This is protected by an Arc so
/// that it can be accessed from both the main screen thread and also the search
/// thread.
struct SearchInner {
    pattern: String,
    kind: SearchKind,
    regex: Regex,
    matches: RwLock<Vec<(usize, usize)>>,
    matching_lines: RwLock<SpanSet>,
    current_match: RwLock<Option<usize>>,
    matching_line_count: AtomicUsize,
    search_line_count: AtomicUsize,
    finished: AtomicBool,
}

/// A search for a pattern within a file.
pub(crate) struct Search {
    inner: Arc<SearchInner>,
}

impl SearchInner {
    /// Create a new SearchInner for a search.
    fn new(
        file: &File,
        pattern: &str,
        kind: SearchKind,
        event_sender: EventSender,
    ) -> Result<Arc<SearchInner>, Error> {
        let regex = Regex::new(pattern)?;
        let search = Arc::new(SearchInner {
            pattern: pattern.to_string(),
            kind,
            regex: regex.clone(),
            matches: RwLock::new(Vec::new()),
            matching_lines: RwLock::new(SpanSet::new()),
            current_match: RwLock::new(None),
            matching_line_count: AtomicUsize::new(0),
            search_line_count: AtomicUsize::new(0),
            finished: AtomicBool::new(false),
        });
        thread::Builder::new()
            .name(String::from("sp-search"))
            .spawn({
                let search = search.clone();
                let file = file.clone();
                move || {
                    let mut matched = false;
                    loop {
                        let loaded = file.loaded();
                        let lines = file.lines();
                        let search_line_count = search.search_line_count.load(Ordering::SeqCst);
                        let search_limit = min(
                            search_line_count + SEARCH_BATCH_SIZE,
                            if loaded { lines } else { lines - 1 },
                        );
                        for line in search_line_count..search_limit {
                            let count = file.with_line(line, |data| {
                                // Strip trailing LF or CRLF if it is there.
                                let len = trim_trailing_newline(&data[..]);
                                let data = overstrike::convert_overstrike(&data[..len]);
                                let data = ESCAPE_SEQUENCE.replace_all(&data[..], NoExpand(b""));
                                regex.find_iter(&data[..]).count()
                            });
                            if count.unwrap_or(0) > 0 {
                                let mut matching_lines = search.matching_lines.write().unwrap();
                                matching_lines.insert(line);
                                let mut matches = search.matches.write().unwrap();
                                let first_match_index = matches.len();
                                for i in 0..count.unwrap() {
                                    matches.push((line, i));
                                }
                                search.matching_line_count.fetch_add(1, Ordering::SeqCst);
                                if !matched {
                                    if let Some(index) = match search.kind {
                                        SearchKind::First => Some(first_match_index),
                                        SearchKind::FirstAfter(offset) => {
                                            if line >= offset {
                                                Some(first_match_index)
                                            } else {
                                                None
                                            }
                                        }
                                        SearchKind::FirstBefore(offset) => {
                                            if line >= offset
                                                && first_match_index > 0
                                                && matches[first_match_index - 1].0 < offset
                                            {
                                                Some(first_match_index - 1)
                                            } else {
                                                None
                                            }
                                        }
                                    } {
                                        *search.current_match.write().unwrap() = Some(index);
                                        event_sender
                                            .send(Event::SearchFirstMatch(file.index()))
                                            .unwrap();
                                        matched = true;
                                    }
                                }
                            }
                        }
                        search
                            .search_line_count
                            .store(search_limit, Ordering::SeqCst);
                        if loaded && search_limit == lines {
                            // Searched the whole file.
                            break;
                        }
                        if !loaded && search_limit >= lines - 1 {
                            // Searched the whole file so far.  Wait for more data.
                            thread::sleep(time::Duration::from_millis(100));
                        }
                    }
                    if !matched {
                        let matches = search.matches.read().unwrap();
                        if matches.len() > 0 {
                            let index = match search.kind {
                                SearchKind::First | SearchKind::FirstAfter(_) => 0,
                                SearchKind::FirstBefore(_) => matches.len() - 1,
                            };
                            *search.current_match.write().unwrap() = Some(index);
                            event_sender
                                .send(Event::SearchFirstMatch(file.index()))
                                .unwrap();
                        }
                    }
                    search.finished.store(true, Ordering::SeqCst);
                    event_sender
                        .send(Event::SearchFinished(file.index()))
                        .unwrap();
                }
            })
            .unwrap();
        Ok(search)
    }
}

impl Search {
    /// Create a new search for a pattern.
    pub(crate) fn new(
        file: &File,
        pattern: &str,
        kind: SearchKind,
        event_sender: EventSender,
    ) -> Result<Search, Error> {
        Ok(Search {
            inner: SearchInner::new(file, pattern, kind, event_sender)?,
        })
    }

    /// Returns true if the search has finished searching the whole file.
    pub(crate) fn finished(&self) -> bool {
        self.inner.finished.load(Ordering::SeqCst)
    }

    /// Renders the search overlay line.
    pub(crate) fn render(&mut self, changes: &mut Vec<Change>, line: usize, width: usize) {
        let mut width = width;
        changes.push(Change::CursorPosition {
            x: Position::Absolute(0),
            y: Position::Absolute(line),
        });
        changes.push(Change::AllAttributes(
            CellAttributes::default()
                .set_foreground(AnsiColor::Black)
                .set_background(AnsiColor::Silver)
                .clone(),
        ));
        if width < 8 {
            // The screen is too small to write anything, just write a blank bar.
            changes.push(Change::ClearToEndOfLine(AnsiColor::Silver.into()));
            return;
        }
        changes.push(Change::Text("  ".into()));
        width -= 2;

        let matches = self.inner.matches.read().unwrap();
        let match_info = match *self.inner.current_match.read().unwrap() {
            Some(index) => Cow::Owned(format!(
                "{} of {} matches on {} lines",
                index + 1,
                matches.len(),
                self.inner.matching_line_count.load(Ordering::SeqCst),
            )),
            _ if self.inner.finished.load(Ordering::SeqCst) => Cow::Borrowed("No matches"),
            _ => Cow::Owned(format!(
                "Searched {} lines",
                self.inner.search_line_count.load(Ordering::SeqCst),
            )),
        };

        // The right-hand side is shown only if it can fit.
        let right_width = match_info.width() + 2;
        let mut left_width = width;
        if width >= right_width {
            left_width -= right_width;
        }

        // Write the left-hand side if it fits.
        match left_width {
            0 => {}
            1 => changes.push(Change::Text(" ".into())),
            _ => changes.push(Change::Text(format!(
                "{1:0$.0$} ",
                left_width - 1,
                self.inner.pattern
            ))),
        }

        // Write the right-hand side if it fits.
        if width >= right_width {
            changes.push(Change::Text(match_info.into()));
            changes.push(Change::ClearToEndOfLine(AnsiColor::Silver.into()));
        }
    }

    /// Returns the line number and match index of the current match.
    pub(crate) fn current_match(&self) -> Option<(usize, usize)> {
        let matches = self.inner.matches.read().unwrap();
        let current_match_index = self.inner.current_match.read().unwrap();
        current_match_index.map(|index| matches[index])
    }

    /// Moves to another match if there is one.
    ///
    /// `scope` describes visible lines of the file on screen.
    /// It is used for `*Screen` movements.
    pub(crate) fn move_match(&mut self, motion: MatchMotion, scope: RangeInclusive<usize>) {
        let matches = self.inner.matches.read().unwrap();
        if matches.len() > 0 {
            let mut current_match_index = self.inner.current_match.write().unwrap();
            if let Some(ref mut index) = *current_match_index {
                // If the current match is within `line_scope`, then `*Screen` is just `*` movement.
                let need_seek = matches!(
                    motion,
                    MatchMotion::NextScreen | MatchMotion::PreviousScreen
                ) && !scope.contains(&matches[*index].0);
                match motion {
                    MatchMotion::First => *index = 0,
                    MatchMotion::PreviousLine => {
                        let match_index = matches[*index].1;
                        if match_index < *index {
                            *index -= match_index + 1;
                        }
                    }
                    MatchMotion::Previous | MatchMotion::PreviousScreen if *index > 0 => {
                        *index -= 1
                    }
                    MatchMotion::Next | MatchMotion::NextScreen if *index < matches.len() - 1 => {
                        *index += 1
                    }
                    MatchMotion::NextLine => {
                        let line_index = matches[*index].0;
                        let mut new_index = *index;
                        while new_index < matches.len() - 1 && matches[new_index].0 == line_index {
                            new_index += 1;
                        }
                        if matches[new_index].0 != line_index {
                            *index = new_index;
                        }
                    }
                    MatchMotion::Last => *index = matches.len() - 1,
                    _ => {}
                }

                // Attempt to satisfy the scope limit.
                if need_seek {
                    match motion {
                        MatchMotion::NextScreen => {
                            let mut candidate_index = *index;
                            if matches[candidate_index].0 > *scope.end() {
                                // Re-search from the beginning.
                                candidate_index = 0;
                            }
                            // Search forward.
                            while candidate_index < matches.len() - 1 {
                                if matches[candidate_index].0 >= *scope.start() {
                                    *index = candidate_index;
                                    break;
                                }
                                candidate_index += 1;
                            }
                        }
                        MatchMotion::PreviousScreen => {
                            let mut candidate_index = *index;
                            if matches[candidate_index].0 < *scope.start() {
                                // Re-search from the end.
                                candidate_index = matches.len() - 1;
                            }
                            // Search backward.
                            while candidate_index > 0 {
                                if matches[candidate_index].0 <= *scope.end() {
                                    *index = candidate_index;
                                    break;
                                }
                                candidate_index -= 1;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    /// Returns the lines in the given range that match.
    pub(crate) fn matching_lines(&self, start: usize, end: usize) -> Vec<usize> {
        let mut lines = Vec::new();
        let matching_lines = self.inner.matching_lines.read().unwrap();
        for line in start..end {
            if matching_lines.contains(line) {
                lines.push(line);
            }
        }
        lines
    }

    /// Returns the number of searched lines.
    pub(crate) fn searched_lines(&self) -> usize {
        self.inner.search_line_count.load(Ordering::SeqCst)
    }

    /// Returns the Regex used for this search.
    pub(crate) fn regex(&self) -> &Regex {
        &self.inner.regex
    }

    /// Returns true if the line index matches the search
    pub(crate) fn line_matches(&self, line_index: usize) -> bool {
        self.inner
            .matching_lines
            .read()
            .unwrap()
            .contains(line_index)
    }
}

pub(crate) fn trim_trailing_newline(data: impl AsRef<[u8]>) -> usize {
    let data = data.as_ref();
    let mut len = data.len();
    if len > 0 && data[len - 1] == b'\n' {
        len -= 1;
        if len > 0 && data[len - 1] == b'\r' {
            len -= 1;
        }
    }
    len
}
