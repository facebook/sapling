/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;

/// Strip horizontal whitespace characters from content for whitespace-insensitive diffs.
///
/// This function removes space, tab, and carriage return characters
/// from the content while preserving newlines. This allows for whitespace-insensitive
/// diffs that still respect line boundaries.
pub fn strip_horizontal_whitespace(bytes: &Bytes) -> Bytes {
    bytes
        .iter()
        .filter(|&&b| b != b' ' && b != b'\t' && b != b'\r')
        .copied()
        .collect::<Vec<_>>()
        .into()
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_no_whitespace() {
        let input = Bytes::from("nowhitespace");
        let output = strip_horizontal_whitespace(&input);
        assert_eq!(output, Bytes::from("nowhitespace"));
    }

    #[mononoke::test]
    fn test_mixed_line_endings() {
        // Test file with mixed CRLF and LF line endings
        let input = Bytes::from("line1\r\nline2\nline3\r\nline4\n");
        let output = strip_horizontal_whitespace(&input);
        // All \r should be removed, leaving only \n
        assert_eq!(output, Bytes::from("line1\nline2\nline3\nline4\n"));
    }

    #[mononoke::test]
    fn test_mixed_line_endings_with_spaces() {
        // Test file with mixed line endings and spaces
        let input = Bytes::from("  line1  \r\n\tline2\t\nline3  \r\n  line4\n");
        let output = strip_horizontal_whitespace(&input);
        // All spaces, tabs, and \r should be removed, leaving only content and \n
        assert_eq!(output, Bytes::from("line1\nline2\nline3\nline4\n"));
    }
}
