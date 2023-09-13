//! Utilities.

use std::borrow::Cow;

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Returns the maximum width in characters of a number.
pub(crate) fn number_width(number: usize) -> usize {
    let mut width = 1;
    let mut limit = 10;
    while limit <= number {
        limit *= 10;
        width += 1;
    }
    width
}

/// Truncates a string to a column offset and width.
pub(crate) fn truncate_string<'a>(
    text: impl Into<Cow<'a, str>>,
    offset: usize,
    width: usize,
) -> String {
    let text = text.into();
    if offset > 0 || width < text.width() {
        let mut column = 0;
        let mut maybe_start_index = None;
        let mut maybe_end_index = None;
        let mut start_pad = 0;
        let mut end_pad = 0;
        for (i, g) in text.grapheme_indices(true) {
            let w = g.width();
            if w != 0 {
                if column >= offset && maybe_start_index.is_none() {
                    maybe_start_index = Some(i);
                    start_pad = column - offset;
                }
                if column + w > offset + width && maybe_end_index.is_none() {
                    maybe_end_index = Some(i);
                    end_pad = offset + width - column;
                    break;
                }
                column += w;
            }
        }
        let start_index = maybe_start_index.unwrap_or(text.len());
        let end_index = maybe_end_index.unwrap_or(text.len());
        format!(
            "{0:1$.1$}{3}{0:2$.2$}",
            "",
            start_pad,
            end_pad,
            &text[start_index..end_index]
        )
    } else {
        text.into_owned()
    }
}
