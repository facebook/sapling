/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

const ENCODED_SLASH: &str = "_SLASH_";
const ENCODED_PLUS: &str = "_PLUS_";
const X_REPO_SEPARATOR: &str = "_TO_";

/// Function responsible for decoding an SM-encoded repo-name.
pub fn decode_repo_name(encoded_repo_name: &str) -> String {
    encoded_repo_name
        .replace(ENCODED_SLASH, "/")
        .replace(ENCODED_PLUS, "+")
}

/// Function responsible for SM-compatible encoding of repo-na
pub fn encode_repo_name(repo_name: &str) -> String {
    repo_name
        .replace('/', ENCODED_SLASH)
        .replace('+', ENCODED_PLUS)
}

/// Function responsible for splitting source and target repo name
/// from combined repo-name string.
pub fn split_repo_names(combined_repo_names: &str) -> Vec<&str> {
    combined_repo_names.split(X_REPO_SEPARATOR).collect()
}
