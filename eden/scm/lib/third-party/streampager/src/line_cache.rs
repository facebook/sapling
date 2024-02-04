//! Line Cache
//!
//! An LRU-cache for lines.

use std::borrow::Cow;
use std::num::NonZeroUsize;

use lru::LruCache;
use regex::bytes::Regex;

use crate::file::{File, FileInfo};
use crate::line::Line;

/// An LRU-cache for Lines.
pub(crate) struct LineCache(LruCache<usize, Line>);

impl LineCache {
    /// Create a new LineCache with the given capacity.
    pub(crate) fn new(capacity: NonZeroUsize) -> LineCache {
        LineCache(LruCache::new(capacity))
    }

    /// Get a line out of the line cache, or create it if it is not
    /// in the cache.
    pub(crate) fn get_or_create<'a>(
        &'a mut self,
        file: &File,
        line_index: usize,
        regex: Option<&Regex>,
    ) -> Option<Cow<'a, Line>> {
        let cache = &mut self.0;
        if cache.contains(&line_index) {
            Some(Cow::Borrowed(cache.get_mut(&line_index).unwrap()))
        } else {
            let line = file.with_line(line_index, |line| {
                if let Some(regex) = regex {
                    Line::new_search(line_index, line, regex)
                } else {
                    Line::new(line_index, line)
                }
            });
            if let Some(line) = line {
                // Don't cache the line if it's the last line of the file
                // and the file is still loading.  It might not be complete.
                if file.loaded() || line_index + 1 < file.lines() {
                    cache.put(line_index, line);
                    Some(Cow::Borrowed(cache.get_mut(&line_index).unwrap()))
                } else {
                    Some(Cow::Owned(line))
                }
            } else {
                None
            }
        }
    }

    /// Clear all entries in the line cache.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}
