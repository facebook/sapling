/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use manifest_tree::PathTranslator;
use types::RepoPath;
use types::RepoPathBuf;

/// Translates between user-facing project paths (`vendor/a`) and their
/// encoded storage representation (`vendor/a{SUFFIX}`). The suffix marks
/// paths that are both a file (submodule) and a directory (containing
/// nested projects) in the tree manifest.
#[derive(Debug)]
pub struct GrepoPathTranslator;

const SUFFIX: &str = "\x7f"; // ASCII code for DEL, impossible in a valid path

impl PathTranslator for GrepoPathTranslator {
    fn encode_file(&self, path: &RepoPath) -> Result<RepoPathBuf> {
        Ok(RepoPathBuf::from_string(format!(
            "{}{}",
            path.as_str(),
            SUFFIX
        ))?)
    }

    fn decode_file(&self, path: &RepoPath) -> Result<RepoPathBuf> {
        let s = path.as_str();
        let decoded = s.strip_suffix(SUFFIX).unwrap_or(s);
        Ok(RepoPathBuf::from_string(decoded.to_string())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_file() {
        let translator = GrepoPathTranslator;
        let path: &RepoPath = "vendor/a".try_into().unwrap();
        let encoded = translator.encode_file(path).unwrap();
        assert_eq!(encoded.as_str(), format!("vendor/a{SUFFIX}"));
    }

    #[test]
    fn test_decode_file_without_suffix() {
        let translator = GrepoPathTranslator;
        let path: &RepoPath = "vendor/a".try_into().unwrap();
        let decoded = translator.decode_file(path).unwrap();
        assert_eq!(decoded.as_str(), "vendor/a");
    }

    #[test]
    fn test_roundtrip() {
        let translator = GrepoPathTranslator;
        let path: &RepoPath = "deep/nested/project".try_into().unwrap();
        let encoded = translator.encode_file(path).unwrap();
        let decoded = translator.decode_file(&encoded).unwrap();
        assert_eq!(decoded.as_str(), path.as_str());
    }
}
