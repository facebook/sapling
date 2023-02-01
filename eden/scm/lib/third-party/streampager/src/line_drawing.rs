//! DEC Line Drawing Mode Handling
//!
//! VT100 and VT220 terminals supported an alternate character set with
//! additional characters, including line drawing characters.  Switching
//! to and from line drawing mode was signalled by escape sequences.
//!
//! Handle this by converting characters between escape sequence blocks
//! into the equivalent unicode character.

// Start replacing bytes after 0x5F.
const REPLACEMENTS_START: usize = 0x5F;

// The bytes starting with 0x5F are replaced with the following unicode strings.
const UNICODE_REPLACEMENTS: &[&str] = &[
    "\u{A0}", "◆", "▒", "␉", "␌", "␍", "␊", "°", "±", "␤", "␋", "┘", "┐", "┌", "└", "┼", "⎺", "⎻",
    "─", "⎼", "⎽", "├", "┤", "┴", "┬", "│", "≤", "≥", "π", "≠", "£", "·",
];

pub(crate) fn convert_line_drawing(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let range = REPLACEMENTS_START..REPLACEMENTS_START + UNICODE_REPLACEMENTS.len();
    for c in input.chars() {
        if range.contains(&(c as usize)) {
            out.push_str(UNICODE_REPLACEMENTS[(c as usize) - REPLACEMENTS_START]);
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_convert_line_drawing() {
        assert_eq!(convert_line_drawing("aaaaa"), "▒▒▒▒▒");
        assert_eq!(convert_line_drawing("tquOKtqu"), "├─┤OK├─┤");
    }
}
