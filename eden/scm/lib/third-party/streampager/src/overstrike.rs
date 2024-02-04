//! Overstrike Handling
//!
//! Typewriter-based terminals used to achieve bold and underlined text by
//! backspacing over the previous character and then overstriking either a copy
//! of the same letter (for bold) or an underscore (for underline). This
//! technique is still in use, in particular for man pages.
//!
//! Handle this by converting runs of overstruck letters into normal text,
//! bracketed by the far more modern SGR escape codes.

use std::borrow::Cow;
use std::str;

use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};

/// An overstrike style.
#[derive(Copy, Clone, PartialEq, Eq)]
enum Overstrike {
    Normal,
    Bold,
    Underline,
    BoldUnderline,
}

impl Overstrike {
    /// Make the overstrike style bold.
    fn bold(&mut self) {
        *self = match *self {
            Overstrike::Normal | Overstrike::Bold => Overstrike::Bold,
            Overstrike::Underline | Overstrike::BoldUnderline => Overstrike::BoldUnderline,
        }
    }

    /// Make the overstrike style underlined.
    fn underline(&mut self) {
        *self = match *self {
            Overstrike::Normal | Overstrike::Underline => Overstrike::Underline,
            Overstrike::Bold | Overstrike::BoldUnderline => Overstrike::BoldUnderline,
        }
    }

    /// Add SGR control sequences to `out` sufficient to switch from the `prev`
    /// overstrike style to this overstrike style.
    fn add_control_sequence(self, prev: Overstrike, out: &mut String) {
        match (prev, self) {
            (Overstrike::Normal, Overstrike::Bold) => out.push_str("\x1B[1m"),
            (Overstrike::Normal, Overstrike::Underline) => out.push_str("\x1B[4m"),
            (Overstrike::Normal, Overstrike::BoldUnderline) => out.push_str("\x1B[1;4m"),
            (Overstrike::Bold, Overstrike::Normal) => out.push_str("\x1B[22m"),
            (Overstrike::Bold, Overstrike::Underline) => out.push_str("\x1B[22;4m"),
            (Overstrike::Bold, Overstrike::BoldUnderline) => out.push_str("\x1B[4m"),
            (Overstrike::Underline, Overstrike::Normal) => out.push_str("\x1B[24m"),
            (Overstrike::Underline, Overstrike::Bold) => out.push_str("\x1B[24;1m"),
            (Overstrike::Underline, Overstrike::BoldUnderline) => out.push_str("\x1B[1m"),
            (Overstrike::BoldUnderline, Overstrike::Normal) => out.push_str("\x1B[22;24m"),
            (Overstrike::BoldUnderline, Overstrike::Bold) => out.push_str("\x1B[24m"),
            (Overstrike::BoldUnderline, Overstrike::Underline) => out.push_str("\x1B[22m"),
            _ => {}
        }
    }
}

/// Erase the last grapheme from the string.  If that's not possible, or if the
/// previous grapheme was a control character, add a backspace character to the
/// string.
fn backspace(out: &mut String) {
    let mut cursor = GraphemeCursor::new(out.len(), out.len(), true);
    if let Ok(Some(offset)) = cursor.prev_boundary(out, 0) {
        if out[offset..]
            .chars()
            .next()
            .map_or(true, char::is_control)
        {
            out.push('\x08');
        } else {
            out.truncate(offset);
        }
    } else {
        out.push('\x08');
    }
}

/// Convert a span of unicode characters with overstrikes into a span with
/// escape sequences
fn convert_unicode_span(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut prev_grapheme = None;
    let mut prev_overstrike = Overstrike::Normal;
    let mut overstrike = Overstrike::Normal;
    let mut graphemes = input.graphemes(true);
    while let Some(grapheme) = graphemes.next() {
        if grapheme == "\x08" {
            if prev_grapheme.is_some() {
                if let Some(next_grapheme) = graphemes.next() {
                    if next_grapheme == "\x08" {
                        backspace(&mut result);
                        prev_grapheme = None;
                        overstrike = Overstrike::Normal;
                    } else if prev_grapheme == Some(next_grapheme) {
                        if next_grapheme == "_" {
                            // Overstriking underscore with itself is
                            // ambiguous.  Prefer to continue the existing
                            // overstrike if there is any.
                            if overstrike == Overstrike::Normal {
                                if prev_overstrike != Overstrike::Normal {
                                    overstrike = prev_overstrike;
                                } else {
                                    overstrike.bold();
                                }
                            } else {
                                overstrike = Overstrike::BoldUnderline;
                            }
                        } else {
                            overstrike.bold()
                        }
                    } else if next_grapheme == "_" {
                        overstrike.underline();
                    } else if prev_grapheme == Some("_") {
                        overstrike.underline();
                        prev_grapheme = Some(next_grapheme);
                    } else {
                        overstrike = Overstrike::Normal;
                        prev_grapheme = Some(next_grapheme);
                    }
                } else {
                    prev_grapheme = None;
                }
            } else {
                backspace(&mut result);
                overstrike = Overstrike::Normal;
            }
        } else {
            if let Some(prev_grapheme) = prev_grapheme {
                overstrike.add_control_sequence(prev_overstrike, &mut result);
                result.push_str(prev_grapheme);
            }
            prev_overstrike = overstrike;
            prev_grapheme = Some(grapheme);
            overstrike = Overstrike::Normal;
        }
    }
    if let Some(prev_grapheme) = prev_grapheme {
        overstrike.add_control_sequence(prev_overstrike, &mut result);
        result.push_str(prev_grapheme);
        prev_overstrike = overstrike;
    }
    Overstrike::Normal.add_control_sequence(prev_overstrike, &mut result);
    result
}

/// Convert any overstrike sequences found in the `input` string into normal
/// text, bracketed by SGR escape sequences.
///
/// For example `"text in b\bbo\bol\bld\bd or l\b_i\b_n\b_e\b_d"` becomes
/// `"text in {bold-on}bold{bold-off} or {ul-on}lined{ul-off}"` (where
/// `\b` is a backspace and the text in braces is the corresponding SGR
/// sequence).
pub(crate) fn convert_overstrike(input: &[u8]) -> Cow<'_, [u8]> {
    if input.contains(&b'\x08') {
        let mut data = Vec::new();
        let mut input = input;
        loop {
            match str::from_utf8(input) {
                Ok(valid) => {
                    data.extend_from_slice(convert_unicode_span(valid).as_bytes());
                    break;
                }
                Err(error) => {
                    let (valid, after_valid) = input.split_at(error.valid_up_to());
                    if !valid.is_empty() {
                        data.extend_from_slice(
                            convert_unicode_span(unsafe { str::from_utf8_unchecked(valid) })
                                .as_bytes(),
                        );
                    }
                    if let Some(len) = error.error_len() {
                        data.extend_from_slice(&after_valid[..len]);
                        input = &after_valid[len..];
                    } else {
                        data.extend_from_slice(after_valid);
                        break;
                    }
                }
            }
        }
        Cow::Owned(data)
    } else {
        Cow::Borrowed(input)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn test_convert_unicode_span() {
        // For simplicity, we will use 'B' as backspace in these tests.
        let bs_re = regex::Regex::new("B").unwrap();
        let bs = move |s| bs_re.replace_all(s, "\x08").to_string();

        assert_eq!(convert_unicode_span("hello"), "hello");
        assert_eq!(
            convert_unicode_span(&bs("_Bh_Be_Bl_Bl_Bo")),
            "\x1B[4mhello\x1B[24m"
        );
        assert_eq!(
            convert_unicode_span(&bs("hBheBelBllBloBo")),
            "\x1B[1mhello\x1B[22m"
        );
        assert_eq!(
            convert_unicode_span(&bs(
                "support bBboBolBldBd, uB_nB__Bd_BérB_lB_íB_nB__Be and bB_BboBoB__BtBthB_BhBh!"
            )),
            "support \x1B[1mbold\x1B[22m, \x1B[4mundérlíne\x1B[24m and \x1B[1;4mboth\x1B[22;24m!"
        );
        assert_eq!(
            convert_unicode_span(&bs("BBxBB can erase bBbBmistayBkes !!BBB.")),
            bs("BBB can erase mistakes.")
        );
        assert_eq!(
            convert_unicode_span(&bs("ambig _B_bBb_B_ _B_uB__B_ bBb_B_ uB__B_B_")),
            "ambig \x1B[1m_b_\x1B[22m \x1B[1m_\x1B[22;4mu_\x1B[24m \x1B[1mb_\x1B[22m \x1B[4mu\x1B[1m_\x1B[22;24m"
        );
        assert_eq!(
            convert_unicode_span(&bs("combining: a\u{301}Ba bBba\u{301}Ba\u{301}tBt bB_a\u{301}B__Ba\u{301}tB_ xa\u{301}a\u{301}BBx")),
            "combining: a \x1B[1mba\u{301}t\x1B[22m \x1B[4mba\u{301}a\u{301}t\x1B[24m xx"
        );
    }
}
