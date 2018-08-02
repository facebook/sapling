use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;
use util::{read_access_token, TOKEN_FILENAME};

#[test]
fn test_read_access_token_from_file_should_return_token() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(TOKEN_FILENAME);
    let mut tmp = File::create(path).unwrap();
    writeln!(tmp, "[commitcloud]");
    writeln!(tmp, "user_token=token");
    let result = read_access_token(&Some(PathBuf::from(dir.path()))).unwrap();
    drop(tmp);
    dir.close();
    assert_eq!(result, "token");
}

#[cfg(target_os = "macos")]
#[test]
fn test_read_access_token_from_keychain_should_return_token() {
    let result = read_access_token(&None).unwrap();
    assert!(!result.is_empty())
}

#[cfg(unix)]
#[cfg(not(target_os = "macos"))]
#[test]
fn test_read_access_token_from_secrets_should_return_token() {
    // Use the secret "COMMITCLOUD_TEST" for testing purposes
    env::set_var("USER", "test");
    let result = read_access_token(&None).unwrap();
    assert_eq!(result, "token");
}
