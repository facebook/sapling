/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Repo tool compatibility
//!
//! This crate provides capabilities to support the
//! [repo tool](https://gerrit.googlesource.com/git-repo) with `.repo/` identity.

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use types::RepoPathBuf;

pub mod commit;
pub mod init;
pub mod path;
pub mod trees;

/// The path of the manifests repo, relative to the repo root.
pub const MANIFESTS_DIR: &str = ".repo/manifests";

/// Return the path of the repo manifest XML in the manifests repo.
///
/// The path comes from the config `grepo.manifestpath`. The default value is
/// `default.xml`.
pub fn repo_xml_path(config: &dyn Config) -> Result<RepoPathBuf> {
    let path: String = config.get_or("grepo", "manifestpath", || "default.xml".to_string())?;
    Ok(RepoPathBuf::from_string(path)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use types::testutil::repo_path_buf;

    use super::*;

    #[test]
    fn test_repo_xml_path_reads_config() -> Result<()> {
        assert_eq!(
            repo_xml_path(&BTreeMap::<&str, &str>::new())?,
            repo_path_buf("default.xml")
        );
        assert_eq!(
            repo_xml_path(&BTreeMap::from([(
                "grepo.manifestpath",
                "static/static.xml"
            )]))?,
            repo_path_buf("static/static.xml")
        );
        Ok(())
    }
}
