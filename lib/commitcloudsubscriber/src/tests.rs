// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::util::{read_access_token, TOKEN_FILENAME};
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

#[test]
fn test_read_access_token_from_file_should_return_token() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(TOKEN_FILENAME);
    let mut tmp = File::create(path).unwrap();
    writeln!(tmp, "[commitcloud]").unwrap();
    writeln!(tmp, "user_token=token").unwrap();
    let result = read_access_token(&Some(PathBuf::from(dir.path()))).unwrap();
    drop(tmp);
    dir.close().unwrap();
    assert_eq!(result.token, "token");
}

#[cfg(target_os = "macos")]
#[test]
// we get panic with CommitCloudUnexpectedError("Token Lookup: token not found")
#[ignore]
fn test_read_access_token_from_keychain_should_return_token() {
    let result = read_access_token(&None).unwrap();
    assert!(!result.token.is_empty())
}

#[cfg(unix)]
#[cfg(not(target_os = "macos"))]
#[test]
// I seem to get a real token from this test
#[ignore]
fn test_read_access_token_from_secrets_should_return_token() {
    // Use the secret "COMMITCLOUD_TEST" for testing purposes
    env::set_var("USER", "test");
    let result = read_access_token(&None).unwrap();
    assert_eq!(result.token, "token");
}
