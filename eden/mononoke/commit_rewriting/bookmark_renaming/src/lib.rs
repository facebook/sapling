/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use ascii::AsciiString;
use bookmarks::BookmarkKey;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::CommonCommitSyncConfig;
use mononoke_types::RepositoryId;
use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("Small repo {0} not found")]
    SmallRepoNotFound(RepositoryId),
}

/// A function to modify bookmark names during the x-repo sync
pub type BookmarkRenamer = Arc<dyn Fn(&BookmarkKey) -> Option<BookmarkKey> + Send + Sync + 'static>;

/// Both forward and reverse `BookmarkRenamer`, encapsulated in a struct
pub struct BookmarkRenamers {
    pub bookmark_renamer: BookmarkRenamer,
    pub reverse_bookmark_renamer: BookmarkRenamer,
}

fn get_prefix_and_common_bookmarks(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<(AsciiString, HashSet<BookmarkKey>)> {
    let common_pushrebase_bookmarks: HashSet<BookmarkKey> = commit_sync_config
        .common_pushrebase_bookmarks
        .iter()
        .cloned()
        .collect();
    let prefix = commit_sync_config
        .small_repos
        .get(&small_repo_id)
        .ok_or(ErrorKind::SmallRepoNotFound(small_repo_id))?
        .bookmark_prefix
        .clone();
    Ok((prefix, common_pushrebase_bookmarks))
}

/// Get a renamer for small-to-large repo sync
pub fn get_small_to_large_renamer(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<BookmarkRenamer> {
    let (prefix, common_pushrebase_bookmarks) =
        get_prefix_and_common_bookmarks(commit_sync_config, small_repo_id)?;
    Ok(Arc::new(move |source_bookmark_name| {
        if common_pushrebase_bookmarks.contains(source_bookmark_name) {
            Some(source_bookmark_name.clone())
        } else {
            let mut prefixed_name = prefix.clone();
            prefixed_name.push_str(source_bookmark_name.as_ascii());
            Some(BookmarkKey::new_ascii(prefixed_name))
        }
    }))
}

/// Get a renamer for a large-to-small repo sync
pub fn get_large_to_small_renamer(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<BookmarkRenamer> {
    let (prefix, common_pushrebase_bookmarks) =
        get_prefix_and_common_bookmarks(commit_sync_config, small_repo_id)?;

    Ok(Arc::new(move |source_bookmark_name| {
        if common_pushrebase_bookmarks.contains(source_bookmark_name) {
            Some(source_bookmark_name.clone())
        } else if source_bookmark_name.as_str().starts_with(prefix.as_str()) {
            let unprefixed = &source_bookmark_name.as_ascii()[prefix.len()..];
            Some(BookmarkKey::new_ascii(unprefixed.into()))
        } else {
            None
        }
    }))
}

/// Get both forward and reverse bookmark renamer in the `BookmarkRenamers` struct
pub fn get_bookmark_renamers(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
    direction: CommitSyncDirection,
) -> Result<BookmarkRenamers> {
    match direction {
        CommitSyncDirection::LargeToSmall => Ok(BookmarkRenamers {
            bookmark_renamer: get_large_to_small_renamer(commit_sync_config, small_repo_id)?,
            reverse_bookmark_renamer: get_small_to_large_renamer(
                commit_sync_config,
                small_repo_id,
            )?,
        }),
        CommitSyncDirection::SmallToLarge => Ok(BookmarkRenamers {
            bookmark_renamer: get_small_to_large_renamer(commit_sync_config, small_repo_id)?,
            reverse_bookmark_renamer: get_large_to_small_renamer(
                commit_sync_config,
                small_repo_id,
            )?,
        }),
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::str::FromStr;

    use maplit::hashmap;
    use metaconfig_types::SmallRepoPermanentConfig;

    use super::*;

    fn get_commit_sync_config() -> CommonCommitSyncConfig {
        CommonCommitSyncConfig {
            common_pushrebase_bookmarks: vec![
                BookmarkKey::new("m1").unwrap(),
                BookmarkKey::new("m2").unwrap(),
            ],
            small_repos: hashmap! {
                RepositoryId::new(1) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("b1/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::new(),
                },
                RepositoryId::new(2) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("b2/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::new(),
                },
            },
            large_repo_id: RepositoryId::new(0),
        }
    }

    #[test]
    fn test_small_to_large_renamer() {
        let commit_sync_config = get_commit_sync_config();
        let bookmark_renamer_1 =
            get_small_to_large_renamer(&commit_sync_config, RepositoryId::new(1)).unwrap();
        let bookmark_renamer_2 =
            get_small_to_large_renamer(&commit_sync_config, RepositoryId::new(2)).unwrap();

        let hello = BookmarkKey::new("hello").unwrap();
        let b1_hello = BookmarkKey::new("b1/hello").unwrap();
        let b2_hello = BookmarkKey::new("b2/hello").unwrap();
        let m1 = BookmarkKey::new("m1").unwrap();
        let m2 = BookmarkKey::new("m2").unwrap();

        assert_eq!(bookmark_renamer_1(&hello), Some(b1_hello));
        assert_eq!(bookmark_renamer_2(&hello), Some(b2_hello));
        assert_eq!(bookmark_renamer_1(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_2(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_1(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_2(&m2), Some(m2.clone()));
    }

    #[test]
    fn test_large_to_small_renamer() {
        let commit_sync_config = get_commit_sync_config();
        let bookmark_renamer_1 =
            get_large_to_small_renamer(&commit_sync_config, RepositoryId::new(1)).unwrap();
        let bookmark_renamer_2 =
            get_large_to_small_renamer(&commit_sync_config, RepositoryId::new(2)).unwrap();

        let hello = BookmarkKey::new("hello").unwrap();
        let b1_hello = BookmarkKey::new("b1/hello").unwrap();
        let b2_hello = BookmarkKey::new("b2/hello").unwrap();
        let m1 = BookmarkKey::new("m1").unwrap();
        let m2 = BookmarkKey::new("m2").unwrap();

        // Unprefixed and non-common-pushrebase bookmarks are not synced
        assert_eq!(bookmark_renamer_1(&hello), None);
        assert_eq!(bookmark_renamer_2(&hello), None);
        // Common-pushrebase bookmarks are synced as is
        assert_eq!(bookmark_renamer_1(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_2(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_1(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_2(&m2), Some(m2.clone()));
        // Correctly prefixed bookmarks are synced with prefixes removed
        assert_eq!(bookmark_renamer_1(&b1_hello), Some(hello.clone()));
        assert_eq!(bookmark_renamer_2(&b2_hello), Some(hello));
        // Bookmarks, prefixed with prefixes, belonging to other small repos are not synced
        assert_eq!(bookmark_renamer_1(&b2_hello), None);
        assert_eq!(bookmark_renamer_2(&b1_hello), None);
    }
}
