/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Git mutation tracking: extract predecessor information from git commit
//! extra headers.
//!
//! When `commit.recordPredecessor` is enabled in the Meta git client, amend,
//! rebase, and cherry-pick operations inject `predecessor` and `predecessor-op`
//! extra headers into the commit object. This module provides types and
//! extraction logic for reading those headers.

use std::str::FromStr;

use mononoke_types::hash::GitSha1;

/// A git mutation entry extracted from commit extra headers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMutationEntry {
    /// The commit(s) that were replaced by this mutation.
    /// Usually one predecessor (amend, rebase, cherry-pick).
    /// Multiple predecessors for fold/squash operations (comma-separated in the header).
    pub predecessors: Vec<GitSha1>,
    /// The operation that created this mutation: "amend", "rebase", "cherry-pick", "fold".
    pub op: String,
}

impl GitMutationEntry {
    /// Returns predecessor SHA1s as raw binary bytes (20 bytes each).
    pub fn predecessor_bytes(&self) -> Vec<Vec<u8>> {
        self.predecessors
            .iter()
            .map(|sha1| sha1.as_ref().to_vec())
            .collect()
    }
}

/// Extract git mutation entries from git extra headers.
///
/// Parses `predecessor` (comma-separated hex SHA1s) and `predecessor-op`
/// headers from the given key-value pairs.
///
/// Returns `None` if no predecessor headers are found.
pub fn extract_mutation_from_headers<K: AsRef<[u8]>, V: AsRef<[u8]>>(
    headers: &[(K, V)],
) -> Option<GitMutationEntry> {
    let mut predecessors = Vec::new();
    let mut op = String::new();

    for (key, value) in headers {
        if key.as_ref() == b"predecessor" {
            if let Ok(value_str) = std::str::from_utf8(value.as_ref()) {
                for hex_sha1 in value_str.split(',') {
                    let hex_sha1 = hex_sha1.trim();
                    if !hex_sha1.is_empty() {
                        if let Ok(sha1) = GitSha1::from_str(hex_sha1) {
                            predecessors.push(sha1);
                        }
                    }
                }
            }
        } else if key.as_ref() == b"predecessor-op" {
            if let Ok(value_str) = std::str::from_utf8(value.as_ref()) {
                op = value_str.to_string();
            }
        }
    }

    if predecessors.is_empty() {
        None
    } else {
        Some(GitMutationEntry { predecessors, op })
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_no_headers() {
        let headers: Vec<(Vec<u8>, Vec<u8>)> = vec![];
        assert_eq!(extract_mutation_from_headers(&headers), None);
    }

    #[mononoke::test]
    fn test_no_predecessor_headers() {
        let headers = vec![
            (b"mergetag".to_vec(), b"some-data".to_vec()),
            (b"gpgsig".to_vec(), b"sig-data".to_vec()),
        ];
        assert_eq!(extract_mutation_from_headers(&headers), None);
    }

    #[mononoke::test]
    fn test_single_predecessor_amend() {
        let sha1_hex = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let headers = vec![
            (b"predecessor".to_vec(), sha1_hex.as_bytes().to_vec()),
            (b"predecessor-op".to_vec(), b"amend".to_vec()),
        ];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 1);
        assert_eq!(entry.predecessors[0], GitSha1::from_str(sha1_hex).unwrap());
        assert_eq!(entry.op, "amend");
    }

    #[mononoke::test]
    fn test_multiple_predecessors_fold() {
        let sha1_a = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let sha1_b = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let value = format!("{sha1_a},{sha1_b}");
        let headers = vec![
            (b"predecessor".to_vec(), value.as_bytes().to_vec()),
            (b"predecessor-op".to_vec(), b"fold".to_vec()),
        ];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 2);
        assert_eq!(entry.predecessors[0], GitSha1::from_str(sha1_a).unwrap());
        assert_eq!(entry.predecessors[1], GitSha1::from_str(sha1_b).unwrap());
        assert_eq!(entry.op, "fold");
    }

    #[mononoke::test]
    fn test_rebase() {
        let sha1 = "1234567890abcdef1234567890abcdef12345678";
        let headers = vec![
            (b"predecessor".to_vec(), sha1.as_bytes().to_vec()),
            (b"predecessor-op".to_vec(), b"rebase".to_vec()),
        ];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 1);
        assert_eq!(entry.op, "rebase");
    }

    #[mononoke::test]
    fn test_cherry_pick() {
        let sha1 = "abcdef1234567890abcdef1234567890abcdef12";
        let headers = vec![
            (b"predecessor".to_vec(), sha1.as_bytes().to_vec()),
            (b"predecessor-op".to_vec(), b"cherry-pick".to_vec()),
        ];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 1);
        assert_eq!(entry.op, "cherry-pick");
    }

    #[mononoke::test]
    fn test_predecessor_without_op() {
        let sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let headers = vec![(b"predecessor".to_vec(), sha1.as_bytes().to_vec())];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 1);
        assert!(entry.op.is_empty());
    }

    #[mononoke::test]
    fn test_headers_intermixed() {
        let sha1 = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let headers = vec![
            (b"mergetag".to_vec(), b"tag-data".to_vec()),
            (b"predecessor".to_vec(), sha1.as_bytes().to_vec()),
            (b"gpgsig".to_vec(), b"sig-data".to_vec()),
            (b"predecessor-op".to_vec(), b"amend".to_vec()),
        ];
        let entry = extract_mutation_from_headers(&headers).unwrap();
        assert_eq!(entry.predecessors.len(), 1);
        assert_eq!(entry.predecessors[0], GitSha1::from_str(sha1).unwrap());
        assert_eq!(entry.op, "amend");
    }

    #[mononoke::test]
    fn test_invalid_hex_skipped() {
        let headers = vec![
            (b"predecessor".to_vec(), b"not-a-valid-hex".to_vec()),
            (b"predecessor-op".to_vec(), b"amend".to_vec()),
        ];
        assert_eq!(extract_mutation_from_headers(&headers), None);
    }
}
