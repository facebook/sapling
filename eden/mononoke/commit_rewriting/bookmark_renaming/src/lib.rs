/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
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

struct ParsedConfig {
    prefix: AsciiString,
    common_pushrebase_bookmarks_map: HashMap<BookmarkKey, BookmarkKey>,
    common_pushrebase_bookmarks: HashSet<BookmarkKey>,
}

fn parse_config(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<ParsedConfig> {
    Ok(ParsedConfig {
        common_pushrebase_bookmarks: commit_sync_config
            .common_pushrebase_bookmarks
            .iter()
            .cloned()
            .collect(),
        common_pushrebase_bookmarks_map: commit_sync_config
            .small_repos
            .get(&small_repo_id)
            .ok_or(ErrorKind::SmallRepoNotFound(small_repo_id))?
            .common_pushrebase_bookmarks_map
            .clone(),
        prefix: commit_sync_config
            .small_repos
            .get(&small_repo_id)
            .ok_or(ErrorKind::SmallRepoNotFound(small_repo_id))?
            .bookmark_prefix
            .clone(),
    })
}

/// Get a renamer for small-to-large repo sync
pub fn get_small_to_large_renamer(
    commit_sync_config: &CommonCommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<BookmarkRenamer> {
    let conf = parse_config(commit_sync_config, small_repo_id)?;
    Ok(Arc::new(move |source_bookmark_name| {
        let rev_map = conf
            .common_pushrebase_bookmarks_map
            .clone()
            .into_iter()
            .map(|(k, v)| (v, k))
            .collect::<HashMap<_, _>>();
        let rev_common_bookmarks = conf
            .common_pushrebase_bookmarks
            .iter()
            .map(|bk| conf.common_pushrebase_bookmarks_map.get(bk).unwrap_or(bk))
            .collect::<HashSet<_>>();
        if rev_common_bookmarks.contains(source_bookmark_name) {
            Some(
                rev_map
                    .get(source_bookmark_name)
                    .unwrap_or(source_bookmark_name)
                    .clone(),
            )
        } else {
            let mut prefixed_name = conf.prefix.clone();
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
    let conf = parse_config(commit_sync_config, small_repo_id)?;

    Ok(Arc::new(move |source_bookmark_name| {
        if conf
            .common_pushrebase_bookmarks
            .contains(source_bookmark_name)
        {
            Some(
                conf.common_pushrebase_bookmarks_map
                    .get(source_bookmark_name)
                    .unwrap_or(source_bookmark_name)
                    .clone(),
            )
        } else if source_bookmark_name
            .as_str()
            .starts_with(conf.prefix.as_str())
        {
            let unprefixed = &source_bookmark_name.as_ascii()[conf.prefix.len()..];
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
        let m1 = BookmarkKey::new("m1").unwrap();
        let m2 = BookmarkKey::new("m2").unwrap();
        let heads_m1 = BookmarkKey::new("heads/m1").unwrap();
        CommonCommitSyncConfig {
            common_pushrebase_bookmarks: vec![m1.clone(), m2.clone()],
            small_repos: hashmap! {
                RepositoryId::new(1) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("b1/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::new(),
                },
                RepositoryId::new(2) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("b2/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::new(),
                },
                RepositoryId::new(3) => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("b3/").unwrap(),
                    common_pushrebase_bookmarks_map: HashMap::from([(m1, heads_m1), (m2.clone(), m2)]),
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
        let bookmark_renamer_3 =
            get_small_to_large_renamer(&commit_sync_config, RepositoryId::new(3)).unwrap();

        let hello = BookmarkKey::new("hello").unwrap();
        let b1_hello = BookmarkKey::new("b1/hello").unwrap();
        let b2_hello = BookmarkKey::new("b2/hello").unwrap();
        let b3_hello = BookmarkKey::new("b3/hello").unwrap();
        let m1 = BookmarkKey::new("m1").unwrap();
        let m2 = BookmarkKey::new("m2").unwrap();
        let heads_m1 = BookmarkKey::new("heads/m1").unwrap();

        assert_eq!(bookmark_renamer_1(&hello), Some(b1_hello));
        assert_eq!(bookmark_renamer_2(&hello), Some(b2_hello));
        assert_eq!(bookmark_renamer_3(&hello), Some(b3_hello));
        assert_eq!(bookmark_renamer_1(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_2(&m1), Some(m1.clone()));
        // Called with the source, which in small to large is the "small repo", so the one with
        // heads if it's a git repo
        assert_eq!(bookmark_renamer_3(&heads_m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_1(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_2(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_3(&m2), Some(m2.clone()));
    }

    #[test]
    fn test_large_to_small_renamer() {
        let commit_sync_config = get_commit_sync_config();
        let bookmark_renamer_1 =
            get_large_to_small_renamer(&commit_sync_config, RepositoryId::new(1)).unwrap();
        let bookmark_renamer_2 =
            get_large_to_small_renamer(&commit_sync_config, RepositoryId::new(2)).unwrap();
        let bookmark_renamer_3 =
            get_large_to_small_renamer(&commit_sync_config, RepositoryId::new(3)).unwrap();

        let hello = BookmarkKey::new("hello").unwrap();
        let b1_hello = BookmarkKey::new("b1/hello").unwrap();
        let b2_hello = BookmarkKey::new("b2/hello").unwrap();
        let b3_hello = BookmarkKey::new("b3/hello").unwrap();
        let m1 = BookmarkKey::new("m1").unwrap();
        let m2 = BookmarkKey::new("m2").unwrap();
        let heads_m1 = BookmarkKey::new("heads/m1").unwrap();

        // Unprefixed and non-common-pushrebase bookmarks are not synced
        assert_eq!(bookmark_renamer_1(&hello), None);
        assert_eq!(bookmark_renamer_2(&hello), None);
        assert_eq!(bookmark_renamer_3(&hello), None);
        // Common-pushrebase bookmarks are synced as is
        assert_eq!(bookmark_renamer_1(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_2(&m1), Some(m1.clone()));
        // Called with source, which in large to small is the "large repo", so the one without
        // heads.
        assert_eq!(bookmark_renamer_3(&m1), Some(heads_m1.clone()));
        assert_eq!(bookmark_renamer_1(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_2(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_3(&m2), Some(m2.clone()));
        // Correctly prefixed bookmarks are synced with prefixes removed
        assert_eq!(bookmark_renamer_1(&b1_hello), Some(hello.clone()));
        assert_eq!(bookmark_renamer_2(&b2_hello), Some(hello.clone()));
        assert_eq!(bookmark_renamer_3(&b3_hello), Some(hello));
        // Bookmarks, prefixed with prefixes, belonging to other small repos are not synced
        assert_eq!(bookmark_renamer_1(&b2_hello), None);
        assert_eq!(bookmark_renamer_2(&b1_hello), None);
    }
}
