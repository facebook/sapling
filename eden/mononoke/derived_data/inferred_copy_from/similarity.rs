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
// Ref: https://fburl.com/tse8f9js
const CHUNK_SIZE: usize = 64;

fn normalize_text(text: &[u8]) -> Vec<&[u8]> {
    text.split(|&b| b == b'\n')
        .flat_map(|line| line.trim_ascii().chunks(CHUNK_SIZE))
        .collect()
}

// Trim leading/trailing whitespaces from each line before comparison
// Return a value between 0.0 (no similarity) and 1.0 (identical)
pub fn estimate_similarity(text1: &[u8], text2: &[u8]) -> Result<f64> {
    if text1.len() >= MAX_FILE_SIZE || text2.len() >= MAX_FILE_SIZE {
        bail!(
            "Files in comparison exceeded the size limit of {} bytes.",
            MAX_FILE_SIZE
        )
    }

    let lines1 = normalize_text(text1);
    let lines2 = normalize_text(text2);

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
        let char_count = text.len();
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
        let text = b"Hello\nWorld";
        assert_eq!(estimate_similarity(text, text).unwrap(), 1.0);
    }

    #[mononoke::test]
    fn test_empty_files() {
        assert_eq!(estimate_similarity(b"", b"").unwrap(), 1.0);
        assert_eq!(estimate_similarity(b"", b"Hello").unwrap(), 0.0);
        assert_eq!(estimate_similarity(b"Hello", b"").unwrap(), 0.0);
    }

    #[mononoke::test]
    fn test_single_character_change() {
        let original = b"Hello World";
        let modified = b"Hello World!";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_eq!(similarity, 0.0); // Line-based diff sees complete replacement
    }

    #[mononoke::test]
    fn test_line_addition() {
        let original = b"Line 1\nLine 2\nLine 3";
        let modified = b"Line 1\nLine 2\nNew Line\nLine 3";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.75);
    }

    #[mononoke::test]
    fn test_line_deletion() {
        let original = b"Line 1\nLine 2\nLine 3\nLine 4";
        let modified = b"Line 1\nLine 3\nLine 4";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.75);
    }

    #[mononoke::test]
    fn test_line_modification() {
        let original = b"def hello():\n    print('Hello')\n    return";
        let modified = b"def hello():\n    print('Hello!')\n    return";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.666667); // 2 out of 3 lines are unchanged
    }

    #[mononoke::test]
    fn test_complete_rewrite() {
        let original = b"This is the original content\nwith multiple lines\nand some text";
        let modified = b"Completely different content\nnothing in common\ntotally new";
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_approx_eq!(similarity, 0.0);
    }

    #[mononoke::test]
    fn test_code_refactoring() {
        let original = br#"
function calculateSum(a, b) {
    return a + b;
}

function main() {
    let result = calculateSum(5, 3);
    console.log(result);
}
"#;
        let modified = br#"
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
        assert_approx_eq!(similarity, 0.5); // Moderate similarity
    }

    #[mononoke::test]
    fn test_whitespace_changes() {
        let original = b"line1\nline2\nline3";
        let modified = b"line1\n  line2\n    line3"; // Added indentation
        let similarity = estimate_similarity(original, modified).unwrap();
        assert_eq!(similarity, 1.0); // Identical since we trim whitespaces
    }

    #[mononoke::test]
    fn test_line_reordering() {
        let original = b"Apple\nBanana\nCherry\nDate";
        let modified = b"Banana\nApple\nDate\nCherry";
        let similarity = estimate_similarity(original, modified).unwrap();
        // Reordering typically shows low similarity in line-based diff
        assert_approx_eq!(similarity, 0.5);
    }

    #[mononoke::test]
    fn test_massive_addition() {
        let original = b"Short file";
        let modified = "Short file\n".to_string() + &"New line\n".repeat(100);
        let similarity = estimate_similarity(original, modified.as_bytes()).unwrap();
        assert_approx_eq!(similarity, 0.012346); // Original content becomes tiny fraction
    }

    #[mononoke::test]
    fn test_file_move_simulation() {
        // Simulate moving a function with minor changes
        let original = br#"
// Other code here
class Helper {
    static process(data) {
        return data.map(x => x * 2);
    }
}
// More code
"#;
        let modified = br#"
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
        assert_approx_eq!(similarity, 0.75);
    }

    #[mononoke::test]
    fn test_newline_endings() {
        // File with vs without trailing newline
        let without_newline = b"line1\nline2\n";
        let with_newline = b"line1\nline2\nline3\n";
        let similarity1 = estimate_similarity(without_newline, with_newline).unwrap();
        assert_approx_eq!(similarity1, 0.666667);

        // Multiple trailing newlines
        let single_newline = b"line1\nline2\n";
        let multiple_newlines = b"line1\nline2\n\n\n";
        let similarity2 = estimate_similarity(single_newline, multiple_newlines).unwrap();
        assert_approx_eq!(similarity2, 1.0);
    }

    #[mononoke::test]
    fn test_large_files_small_changes() {
        let mut lines = (0..2500).map(|i| i.to_string()).collect::<Vec<_>>();
        let original = lines.join("\n");
        // Alter some lines
        lines.insert(100, "line1".to_string());
        lines.remove(200);
        lines.remove(300);
        lines.remove(400);
        lines.insert(1000, "line2".to_string());
        lines.remove(1100);
        lines.insert(1500, "line3".to_string());
        lines.insert(2300, "line4".to_string());
        let modified = lines.join("\n");
        let similarity = estimate_similarity(original.as_bytes(), modified.as_bytes()).unwrap();
        assert_approx_eq!(similarity, 0.9984);
    }
}
