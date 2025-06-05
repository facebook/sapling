/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use similar::ChangeTag;
use similar::TextDiff;

// Ref: https://git-scm.com/docs/git-config core.bigFileThreshold
const MAX_FILE_SIZE: usize = 512 * 1024 * 1024; // 512MB

// Trim leading/trailing whitespaces from each line before comparison
// Return a value between 0.0 (no similarity) and 1.0 (identical)
#[allow(unused)]
pub fn estimate_similarity(text1: &str, text2: &str) -> Result<f64> {
    if text1.len() >= MAX_FILE_SIZE || text2.len() >= MAX_FILE_SIZE {
        bail!(
            "Files in comparison exceeded the size limit of {} bytes.",
            MAX_FILE_SIZE
        )
    }

    let lines1: Vec<&str> = text1.lines().map(|line| line.trim()).collect();
    let lines2: Vec<&str> = text2.lines().map(|line| line.trim()).collect();

    if lines1 == lines2 {
        return Ok(1.0);
    }
    if lines1.is_empty() || lines2.is_empty() {
        return Ok(0.0);
    }

    let total_lines = std::cmp::max(lines1.len(), lines2.len());
    let mut unchanged_lines = 0;
    let mut total_chars = 0;
    let mut unchanged_chars = 0;

    let diff = TextDiff::from_slices(&lines1, &lines2);
    for change in diff.iter_all_changes() {
        let text = change.value();
        let char_count = text.len() + 1; // +1 for the implicit newline
        total_chars += char_count;
        if change.tag() == ChangeTag::Equal {
            unchanged_chars += char_count;
            unchanged_lines += 1;
        }
    }

    if total_chars == 0 || total_lines == 0 {
        return Ok(1.0);
    }

    let ratio1 = unchanged_chars as f64 / total_chars as f64;
    let ratio2 = unchanged_lines as f64 / total_lines as f64;
    Ok(ratio1.max(ratio2))
}

#[cfg(test)]
mod tests {
    use assert_approx_eq::assert_approx_eq;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_identical_files() {
        let text = "Hello\nWorld";
        assert_eq!(estimate_similarity(text, text).unwrap(), 1.0);
    }

    #[mononoke::test]
    fn test_empty_files() {
        assert_eq!(estimate_similarity("", "").unwrap(), 1.0);
        assert_eq!(estimate_similarity("", "Hello").unwrap(), 0.0);
        assert_eq!(estimate_similarity("Hello", "").unwrap(), 0.0);
    }

    #[mononoke::test]
    fn test_single_character_change() {
        let original = "Hello World";
        let modified = "Hello World!";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_eq!(similarity, 0.0); // Line-based diff sees complete replacement
    }

    #[mononoke::test]
    fn test_line_addition() {
        let original = "Line 1\nLine 2\nLine 3";
        let modified = "Line 1\nLine 2\nNew Line\nLine 3";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.75);
    }

    #[mononoke::test]
    fn test_line_deletion() {
        let original = "Line 1\nLine 2\nLine 3\nLine 4";
        let modified = "Line 1\nLine 3\nLine 4";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.75);
    }

    #[mononoke::test]
    fn test_line_modification() {
        let original = "def hello():\n    print('Hello')\n    return";
        let modified = "def hello():\n    print('Hello!')\n    return";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.666667); // 2 out of 3 lines are unchanged
    }

    #[mononoke::test]
    fn test_complete_rewrite() {
        let original = "This is the original content\nwith multiple lines\nand some text";
        let modified = "Completely different content\nnothing in common\ntotally new";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.0);
    }

    #[mononoke::test]
    fn test_code_refactoring() {
        let original = r#"
function calculateSum(a, b) {
    return a + b;
}

function main() {
    let result = calculateSum(5, 3);
    console.log(result);
}
"#;
        let modified = r#"
function calculateSum(x, y) {
    if (typeof x !== 'number' || typeof y !== 'number') {
        throw new Error('Invalid input');
    }
    return x + y;
}

function main() {
    let result = calculateSum(5, 3);
    console.log(result);
}
"#;
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.583333); // Moderate similarity
    }

    #[mononoke::test]
    fn test_whitespace_changes() {
        let original = "line1\nline2\nline3";
        let modified = "line1\n  line2\n    line3"; // Added indentation
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_eq!(similarity, 1.0); // Identical since we trim whitespaces
    }

    #[mononoke::test]
    fn test_line_reordering() {
        let original = "Apple\nBanana\nCherry\nDate";
        let modified = "Banana\nApple\nDate\nCherry";
        let similarity = estimate_similarity(original, modified).unwrap();
        // Reordering typically shows low similarity in line-based diff
        assert_approx_eq!(similarity, 0.5);
    }

    #[mononoke::test]
    fn test_massive_addition() {
        let original = "Short file";
        let modified = "Short file\n".to_string() + &"New line\n".repeat(100);
        let similarity = estimate_similarity(original, &modified).unwrap();
        assert_approx_eq!(similarity, 0.012075); // Original content becomes tiny fraction
    }

    #[mononoke::test]
    fn test_file_move_simulation() {
        // Simulate moving a function with minor changes
        let original = r#"
// Other code here
class Helper {
    static process(data) {
        return data.map(x => x * 2);
    }
}
// More code
"#;
        let modified = r#"
// Other code here  
// More code
class Helper {
    static process(data) {
        // Added comment
        return data.map(x => x * 2);
    }
}
"#;
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.777778);
    }

    #[mononoke::test]
    fn test_newline_endings() {
        // File with vs without trailing newline
        let without_newline = "line1\nline2\n";
        let with_newline = "line1\nline2\nline3\n";
        let similarity1 = estimate_similarity(without_newline, with_newline).unwrap();
        assert_approx_eq!(similarity1, 0.666667);

        // Multiple trailing newlines
        let single_newline = "line1\nline2\n";
        let multiple_newlines = "line1\nline2\n\n\n";
        let similarity2 = estimate_similarity(single_newline, multiple_newlines).unwrap();
        assert_approx_eq!(similarity2, 0.857143);
    }
}
