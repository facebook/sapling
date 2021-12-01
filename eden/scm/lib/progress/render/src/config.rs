/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Progress rendering configuration.

use std::borrow::Cow;
use std::time::Duration;

use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub struct RenderingConfig {
    /// Delay before showing a newly created bar.
    pub delay: Duration,

    /// Maximum number of bars to show.
    pub max_bar_count: usize,

    /// Terminal width.
    pub term_width: usize,

    /// Use CJK width (some characters are treated as 2-char width, instead of 1-char width).
    /// Practically, some CJK fonts would work better with this set to true.
    pub cjk_width: bool,
}

impl Default for RenderingConfig {
    fn default() -> Self {
        Self {
            delay: Duration::from_secs(3),
            max_bar_count: 8,
            term_width: 80,
            cjk_width: false,
        }
    }
}

#[cfg(test)]
impl RenderingConfig {
    pub fn for_testing() -> Self {
        Self {
            delay: Duration::from_secs(0),
            max_bar_count: 5,
            term_width: 60,
            cjk_width: false,
        }
    }
}

impl RenderingConfig {
    /// Truncate a single line.
    pub(crate) fn truncate_line<'a>(&self, line: &'a str) -> Cow<'a, str> {
        self.truncate_by_width(line, self.term_width, "…")
    }

    /// Truncate `text` to fit in the given width.
    /// `suffix` is appended if `text` is truncated.
    pub(crate) fn truncate_by_width<'a>(
        &self,
        text: &'a str,
        width: usize,
        suffix: &str,
    ) -> Cow<'a, str> {
        if self.width_str(text) >= width {
            let mut current_width = 0;
            let suffix_width = self.width_str(suffix);
            for (i, ch) in text.char_indices() {
                let next_width = current_width + self.width_char(ch);
                if next_width + suffix_width >= width {
                    // Cannot take this char.
                    return format!("{}{}", &text[..i], suffix).into();
                }
                current_width = next_width;
            }
        }
        return Cow::Borrowed(text);
    }

    pub(crate) fn max_topic_len(&self) -> usize {
        if self.term_width < 80 { 12 } else { 16 }
    }

    fn width_str(&self, text: &str) -> usize {
        if self.cjk_width {
            text.width_cjk()
        } else {
            text.width()
        }
    }

    fn width_char(&self, ch: char) -> usize {
        if self.cjk_width {
            ch.width_cjk()
        } else {
            ch.width()
        }
        .unwrap_or_default()
    }
}
