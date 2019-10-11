/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use ascii::AsciiString;
use bookmarks::BookmarkName;
use failure_ext::prelude::*;
use metaconfig_types::CommitSyncConfig;
use std::collections::HashSet;
use std::iter::Iterator;
use std::sync::Arc;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(display = "Small repo {} not found", _0)]
    SmallRepoNotFound(i32),
}

pub type BookmarkRenamer =
    Arc<dyn Fn(&BookmarkName) -> Option<BookmarkName> + Send + Sync + 'static>;

fn get_prefix_and_common_bookmarks(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: i32,
) -> Result<(AsciiString, HashSet<BookmarkName>)> {
    let common_pushrebase_bookmarks: HashSet<BookmarkName> = commit_sync_config
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
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: i32,
) -> Result<BookmarkRenamer> {
    let (prefix, common_pushrebase_bookmarks) =
        get_prefix_and_common_bookmarks(commit_sync_config, small_repo_id)?;
    Ok(Arc::new(move |source_bookmark_name| {
        if common_pushrebase_bookmarks.contains(source_bookmark_name) {
            Some(source_bookmark_name.clone())
        } else {
            let mut prefixed_name = prefix.clone();
            prefixed_name.push_str(source_bookmark_name.as_ascii());
            Some(BookmarkName::new_ascii(prefixed_name))
        }
    }))
}

/// Get a renamer for a large-to-small repo sync
pub fn get_large_to_small_renamer(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: i32,
) -> Result<BookmarkRenamer> {
    let (prefix, common_pushrebase_bookmarks) =
        get_prefix_and_common_bookmarks(commit_sync_config, small_repo_id)?;

    Ok(Arc::new(move |source_bookmark_name| {
        if common_pushrebase_bookmarks.contains(source_bookmark_name) {
            Some(source_bookmark_name.clone())
        } else if source_bookmark_name.as_str().starts_with(prefix.as_str()) {
            let unprefixed = &source_bookmark_name.as_ascii()[prefix.len()..];
            Some(BookmarkName::new_ascii(unprefixed.into()))
        } else {
            None
        }
    }))
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashmap;
    use mercurial_types::MPath;
    use metaconfig_types::{
        CommitSyncDirection, DefaultSmallToLargeCommitSyncPathAction, SmallRepoCommitSyncConfig,
    };

    fn mp(s: &'static str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn get_small_repo_sync_config_1() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
            map: hashmap! {},
            bookmark_prefix: AsciiString::from_ascii("b1/".to_string()).unwrap(),
        }
    }

    fn get_small_repo_sync_config_2() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted")),
            map: hashmap! {},
            bookmark_prefix: AsciiString::from_ascii("b2/".to_string()).unwrap(),
        }
    }

    fn get_commit_sync_config() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: 3,
            direction: CommitSyncDirection::LargeToSmall,
            common_pushrebase_bookmarks: vec![
                BookmarkName::new("m1").unwrap(),
                BookmarkName::new("m2").unwrap(),
            ],
            small_repos: hashmap! {
                1 => get_small_repo_sync_config_1(),
                2 => get_small_repo_sync_config_2(),
            },
        }
    }

    #[test]
    fn test_small_to_large_renamer() {
        let commit_sync_config = get_commit_sync_config();
        let bookmark_renamer_1 = get_small_to_large_renamer(&commit_sync_config, 1).unwrap();
        let bookmark_renamer_2 = get_small_to_large_renamer(&commit_sync_config, 2).unwrap();

        let hello = BookmarkName::new("hello").unwrap();
        let b1_hello = BookmarkName::new("b1/hello").unwrap();
        let b2_hello = BookmarkName::new("b2/hello").unwrap();
        let m1 = BookmarkName::new("m1").unwrap();
        let m2 = BookmarkName::new("m2").unwrap();

        assert_eq!(bookmark_renamer_1(&hello), Some(b1_hello.clone()));
        assert_eq!(bookmark_renamer_2(&hello), Some(b2_hello.clone()));
        assert_eq!(bookmark_renamer_1(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_2(&m1), Some(m1.clone()));
        assert_eq!(bookmark_renamer_1(&m2), Some(m2.clone()));
        assert_eq!(bookmark_renamer_2(&m2), Some(m2.clone()));
    }

    #[test]
    fn test_large_to_small_renamer() {
        let commit_sync_config = get_commit_sync_config();
        let bookmark_renamer_1 = get_large_to_small_renamer(&commit_sync_config, 1).unwrap();
        let bookmark_renamer_2 = get_large_to_small_renamer(&commit_sync_config, 2).unwrap();

        let hello = BookmarkName::new("hello").unwrap();
        let b1_hello = BookmarkName::new("b1/hello").unwrap();
        let b2_hello = BookmarkName::new("b2/hello").unwrap();
        let m1 = BookmarkName::new("m1").unwrap();
        let m2 = BookmarkName::new("m2").unwrap();

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
        assert_eq!(bookmark_renamer_2(&b2_hello), Some(hello.clone()));
        // Bookmarks, prefixed with prefixes, belonging to other small repos are not synced
        assert_eq!(bookmark_renamer_1(&b2_hello), None);
        assert_eq!(bookmark_renamer_2(&b1_hello), None);
    }
}
