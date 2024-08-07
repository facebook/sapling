/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use tempfile::tempdir;

use crate::util::read_or_generate_access_token;
use crate::util::read_subscriptions;
use crate::util::TOKEN_FILENAME;

#[test]
fn test_read_access_token_from_file_should_return_token() {
    let dir = tempdir().unwrap();
    let path = dir.path().join(TOKEN_FILENAME);
    let mut tmp = File::create(path).unwrap();
    writeln!(tmp, "[commitcloud]").unwrap();
    writeln!(tmp, "user_token=token").unwrap();
    let result = read_or_generate_access_token(&Some(PathBuf::from(dir.path()))).unwrap();
    drop(tmp);
    dir.close().unwrap();
    assert_eq!(result.token, "token");
}

#[test]
fn test_read_subscriptions() -> Result<()> {
    let dir = tempdir()?;

    let repo = dir.path().join("my_repo");
    std::fs::create_dir(&repo)?;

    let joined_dir = dir.path().join(".commitcloud").join("joined");
    std::fs::create_dir_all(&joined_dir)?;

    std::fs::write(
        joined_dir.join("my_sub"),
        format!(
            "[commitcloud]
workspace=my_workspace
repo_name=my_repo
repo_root={}",
            repo.to_str().unwrap()
        ),
    )?;

    let got = read_subscriptions(dir.path())?;
    assert_eq!(got.len(), 1);

    let (sub, paths) = got.into_iter().next().unwrap();
    assert_eq!(sub.repo_name, "my_repo");
    assert_eq!(sub.workspace, "my_workspace");
    assert_eq!(paths, vec![repo]);

    Ok(())
}
