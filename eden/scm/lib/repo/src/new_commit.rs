/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use anyhow::bail;
use format_util::HgCommitFields;
use format_util::hg_sha1_digest;
use minibytes::Text;
use serde::Deserialize;
use types::HgId;

#[derive(Deserialize)]
pub struct NewCommit {
    pub commit_fields: HgCommitFields,
    #[serde(default)]
    pub parents: Vec<HgId>,
    #[serde(default)]
    pub gpg_keyid: Option<String>,
}

impl NewCommit {
    pub fn into_hg_text_node_pair(mut self) -> Result<(Vec<u8>, HgId)> {
        self.commit_fields.author = validated_author(self.commit_fields.author)?;
        self.commit_fields.message = stripped_message(self.commit_fields.message);

        // We no longer support "branch" in the commit message, so we just filter it
        self.commit_fields.extras.remove("branch");

        self.commit_fields.files.sort();

        let text_bytes = self.commit_fields.to_text()?.into_bytes();

        let mut iter = self.parents.into_iter();
        let p1 = iter.next().unwrap_or_default();
        let p2 = iter.next().unwrap_or_default();

        let node = hg_sha1_digest(&text_bytes, &p1, &p2);
        Ok((text_bytes, node))
    }
}

/// Strip trailing whitespace from each line and leading/trailing empty lines from the description.
/// This function allocates only when line trimming is needed
fn stripped_message(desc: Text) -> Text {
    Text::from(
        desc.as_ref()
            .lines()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim_matches('\n') // After trimming lines, we may have created new leading/trailing empty lines
            .to_string(),
    )
}

fn validated_author(user: Text) -> Result<Text> {
    if user.contains('\n') {
        bail!(
            "username '{}' contains a newline",
            user.replace('\n', "\\n")
        );
    }
    if user.trim().is_empty() {
        bail!("empty username");
    }
    Ok(user.slice_to_bytes(user.trim()))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use format_util::HgTime;
    use types::Id20;

    use super::*;

    fn make_commit(description: &str, user: &str, parents: Vec<HgId>) -> NewCommit {
        NewCommit {
            commit_fields: HgCommitFields {
                tree: Id20::from_hex(b"98edb6a9c7a48cae7a1ed9a39600952547daaebb").unwrap(),
                files: vec![],
                message: Text::from(description.to_owned()),
                author: Text::from(user.to_owned()),
                date: HgTime {
                    unixtime: 1714100000,
                    offset: 0,
                },
                extras: BTreeMap::new(),
            },
            parents,
            gpg_keyid: None,
        }
    }

    #[test]
    fn test_to_hg_text_with_two_parents() {
        let parent1 = HgId::from_hex(b"1111111111111111111111111111111111111111").unwrap();
        let parent2 = HgId::from_hex(b"2222222222222222222222222222222222222222").unwrap();
        let commit = make_commit(
            "Merge commit",
            "User <user@example.com>",
            vec![parent1, parent2],
        );

        let (text, node) = commit.into_hg_text_node_pair().unwrap();
        assert!(!text.is_empty());
        assert!(!node.is_null());

        let parent3 = HgId::from_hex(b"3333333333333333333333333333333333333333").unwrap();
        let commit_different_parents = make_commit(
            "Merge commit",
            "User <user@example.com>",
            vec![parent1, parent3],
        );
        let (text_different, node_different) =
            commit_different_parents.into_hg_text_node_pair().unwrap();
        assert_eq!(text, text_different);
        assert_ne!(node, node_different);
    }

    #[test]
    fn test_to_hg_text_trims_user_whitespace() {
        let commit = make_commit("Test", "  User <user@example.com>  ", vec![]);

        let (text, _node) = commit.into_hg_text_node_pair().unwrap();
        let text_str = String::from_utf8(text).unwrap();

        assert!(text_str.contains("\nUser <user@example.com>\n"));
        assert!(!text_str.contains("  User"));
    }

    #[test]
    fn test_stripped_desc_removes_trailing_whitespace_from_lines() {
        assert_eq!(
            stripped_message(Text::from("line1  \nline2\t\nline3   ")),
            Text::from("line1\nline2\nline3")
        );
    }

    #[test]
    fn test_stripped_desc_removes_leading_empty_lines() {
        assert_eq!(
            stripped_message(Text::from("\n\n\nFirst line\nSecond line")),
            Text::from("First line\nSecond line")
        );
    }

    #[test]
    fn test_stripped_desc_removes_trailing_empty_lines() {
        assert_eq!(
            stripped_message(Text::from("First line\nSecond line\n\n\n")),
            Text::from("First line\nSecond line")
        );
    }

    #[test]
    fn test_stripped_desc_removes_both_leading_and_trailing_empty_lines() {
        assert_eq!(
            stripped_message(Text::from("\n\nContent\n\n")),
            Text::from("Content")
        );
    }

    #[test]
    fn test_stripped_desc_preserves_middle_empty_lines() {
        assert_eq!(
            stripped_message(Text::from("First\n\n\nSecond")),
            Text::from("First\n\n\nSecond")
        );
    }

    #[test]
    fn test_stripped_desc_handles_empty_string() {
        assert_eq!(stripped_message(Text::from("")), Text::from(""));
    }

    #[test]
    fn test_stripped_desc_handles_only_whitespace() {
        assert_eq!(stripped_message(Text::from("   \n\t\n   ")), Text::from(""));
    }

    #[test]
    fn test_stripped_desc_handles_single_line_no_whitespace() {
        assert_eq!(
            stripped_message(Text::from("Hello world")),
            Text::from("Hello world")
        );
    }

    #[test]
    fn test_stripped_desc_handles_single_line_with_trailing_whitespace() {
        assert_eq!(
            stripped_message(Text::from("Hello world   ")),
            Text::from("Hello world")
        );
    }

    #[test]
    fn test_stripped_desc_preserves_leading_whitespace_on_lines() {
        assert_eq!(
            stripped_message(Text::from("  indented\n    more indented")),
            Text::from("  indented\n    more indented")
        );
    }

    #[test]
    fn test_stripped_desc_combined_behavior() {
        let input = "\n\n  First line  \n  Second line\t\n\nThird line   \n\n";
        let expected = "  First line\n  Second line\n\nThird line";
        assert_eq!(stripped_message(Text::from(input)), Text::from(expected));
    }

    #[test]
    fn test_stripped_desc_carriage_return_as_content() {
        // Standalone \r is treated as content, not a line separator
        let input = "hello\rworld";
        let result = stripped_message(Text::from(input));
        // \r is kept as content (trim_end will remove trailing \r though)
        assert_eq!(result, Text::from("hello\rworld"));
    }

    #[test]
    fn test_stripped_desc_single_carriage_return() {
        let input = "\r";
        let result = stripped_message(Text::from(input));
        assert_eq!(result, Text::from(""));
    }

    #[test]
    fn test_stripped_desc_crlf_line_ending() {
        let input = "hello\r\nworld\r\n";
        let result = stripped_message(Text::from(input));
        assert_eq!(result, Text::from("hello\nworld"));
    }

    #[test]
    fn test_stripped_desc_only_newlines() {
        // Edge case: string with only \n characters
        // Python: "\n\n\n".splitlines() -> ["", "", ""] -> "\n\n" -> ""
        // Rust: After fix, correctly returns empty string
        let input = "\n\n\n";
        let result = stripped_message(Text::from(input));
        assert_eq!(result, Text::from(""));
    }

    #[test]
    fn test_stripped_desc_trailing_cr_after_content() {
        let input = "hello\r";
        let result = stripped_message(Text::from(input));
        assert_eq!(result, Text::from("hello"));
    }

    #[test]
    fn test_stripped_desc_multiple_cr() {
        let input = "\r\r";
        let result = stripped_message(Text::from(input));
        assert_eq!(result, Text::from(""));
    }

    #[test]
    fn test_to_hg_text_empty_username() {
        let commit = make_commit("Test", "", vec![]);

        let result = commit.into_hg_text_node_pair();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty username"));
    }

    #[test]
    fn test_to_hg_text_username_with_newline() {
        let commit = make_commit("Test", "User\n<user@example.com>", vec![]);

        let result = commit.into_hg_text_node_pair();
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("contains a newline")
        );
    }
}
