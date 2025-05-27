/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use mercurial_types::MPathElement;
use mercurial_types::NonRootMPath;
use metaconfig_types::CommitSyncConfig;
use metaconfig_types::CommitSyncDirection;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use metaconfig_types::SmallRepoCommitSyncConfig;
use mononoke_types::RepositoryId;
use thiserror::Error;

// NOTE: Occurrences of Option<NonRootMPath> in this file have not been replaced with MPath since such a
// replacement is only possible in cases where Option<NonRootMPath> is used to represent a path that can also
// be root. However, in this case the Some(_) and None variant of Option<NonRootMPath> are used to represent
// conditional logic, i.e. the code either does something or skips it based on None or Some.

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Cannot remove prefix, equal to the whole path")]
    RemovePrefixWholePathFailure,
    #[error("Cannot apply prefix action {0:?} to {1:?}")]
    PrefixActionFailure(PrefixAction, NonRootMPath),
    #[error("Small repo {0} not found")]
    SmallRepoNotFound(RepositoryId),
    #[error("Provided map is not prefix-free (e.g. {0:?} and {1:?})")]
    NonPrefixFreeMap(NonRootMPath, NonRootMPath),
}

pub trait Mover: Send + Sync + 'static {
    /// Move a path during cross-repo sync.
    ///
    /// The return values mean:
    /// - `Ok(Some(new_path))` - The path should be replaced with `newpath`.
    /// - `Ok(None)` - The path should not be synced
    /// - `Err(e)` - The sync should fail, as this function could not figure
    ///   out how to rewrite the path.
    ///
    /// NOTE: The return type is `Option<NonRootMPath>`, but `None` means do
    /// not sync, rather than the root path.
    fn move_path(&self, source_path: &NonRootMPath) -> Result<Option<NonRootMPath>>;

    /// Returns true if the path conflicts with any of the paths
    /// the mover will move.  Paths conflict if either one of them
    /// is a path prefix of the other.
    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool>;
}

/// A struct to contain forward and reverse `Mover`
#[derive(Clone)]
pub struct Movers {
    pub mover: Arc<dyn Mover>,
    pub reverse_mover: Arc<dyn Mover>,
}

/// An action, configured for a given prefix
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixAction {
    // The new path should have this prefix replaced with a new value
    Change(NonRootMPath),
    // The new path should have this prefix dropped
    RemovePrefix,
    // The path that matches this prefix should not be synced
    DoNotSync,
}

/// An action, applied to the entire path
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathAction {
    // Change the path when syncing
    Change(NonRootMPath),
    // Do not sync this path
    DoNotSync,
}

/// Default action to apply to a path when syncing between two repos
#[derive(Debug, Clone)]
pub enum DefaultAction {
    /// Prepend path with this prefix
    PrependPrefix(NonRootMPath),
    /// Keep the path as is
    Preserve,
    /// Do not sync this path
    DoNotSync,
}

impl DefaultAction {
    /// Create `DefaultAction` for small-to-large sync
    fn from_default_small_repo_action(dsra: DefaultSmallToLargeCommitSyncPathAction) -> Self {
        match dsra {
            DefaultSmallToLargeCommitSyncPathAction::Preserve => DefaultAction::Preserve,
            DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mpath) => {
                DefaultAction::PrependPrefix(mpath)
            }
        }
    }
}

fn get_suffix_after<'a, 'b>(
    source_path: &'a NonRootMPath,
    candidate_prefix: &'b NonRootMPath,
) -> Option<Vec<&'a MPathElement>> {
    if !candidate_prefix.is_prefix_of(source_path) {
        None
    } else {
        Some(
            source_path
                .into_iter()
                .skip(candidate_prefix.num_components())
                .collect(),
        )
    }
}

/// Given the remainder of the path after a matching prefix
/// and a prefix action, produce a path action
fn get_path_action<'a, I: IntoIterator<Item = &'a MPathElement>>(
    source_path_minus_prefix: I,
    prefix_action: &PrefixAction,
) -> Result<PathAction> {
    match prefix_action {
        PrefixAction::DoNotSync => Ok(PathAction::DoNotSync),
        PrefixAction::RemovePrefix => {
            let elements: Vec<_> = source_path_minus_prefix.into_iter().cloned().collect();
            NonRootMPath::try_from(elements)
                .map(PathAction::Change)
                .map_err(|_| {
                    // This case means that we are trying to sync a file
                    // and are also asked to drop the entire path of this
                    // file.
                    // Note that `PrefixAction::RemovePrefix` can only be
                    // created in this module, and is only ever created
                    // as a reversal of `PrependPrefix` default action,
                    // when configuring sync from large to small repos.
                    // Therefore, this case can only be hit if the large
                    // repo contains a file, named exactly like the
                    // prefix in a `DefaultAction::PrependPrefix` and
                    // this is a mistake (either configuration or somebody
                    // checked in a file named like this).
                    // TODO(ikostia, T53963059): large repo should prohibit such files
                    Error::from(ErrorKind::RemovePrefixWholePathFailure)
                })
        }
        PrefixAction::Change(replacement_prefix) => Ok(PathAction::Change(
            replacement_prefix.join(source_path_minus_prefix),
        )),
    }
}

pub struct CrossRepoMover {
    /// Map of prefixes to their actions, sorted by longest prefix first.
    prefix_map: Vec<(NonRootMPath, PrefixAction)>,
    /// The default action to apply to paths that don't match any prefix.
    default_action: DefaultAction,
    /// A reverse mover to check against,
    reverse_mover: Option<Arc<dyn Mover>>,
}

impl CrossRepoMover {
    /// Create a `Mover`, given a path prefix map and a default action
    pub fn new(
        prefix_map: HashMap<NonRootMPath, PrefixAction>,
        default_action: DefaultAction,
    ) -> Result<Self> {
        // We want `prefix_map` to be ordered longest-to-shortest
        // to allow non-prefix-free maps in the future. For these kinds
        // of maps, we need to ensure we always try to match the longest
        // prefix first, as it's more specific.
        let prefix_map: Vec<(NonRootMPath, PrefixAction)> = {
            let mut v: Vec<(NonRootMPath, PrefixAction)> = prefix_map.into_iter().collect();
            v.sort_unstable_by_key(|(mpath, _)| mpath.len());
            v.reverse();
            v
        };
        Ok(CrossRepoMover {
            prefix_map,
            default_action,
            reverse_mover: None,
        })
    }

    /// Create a `Mover`, given a path prefix map and a default action.
    /// The mover will also check that the result of the move is the same
    /// as the result of applying the reverse mover, and filter out any
    /// paths that don't match the reverse mover.
    pub fn new_with_reverse_mover_check(
        prefix_map: HashMap<NonRootMPath, PrefixAction>,
        default_action: DefaultAction,
        reverse_mover: Arc<dyn Mover>,
    ) -> Result<Self> {
        // We want `prefix_map` to be ordered longest-to-shortest
        // to allow non-prefix-free maps in the future. For these kinds
        // of maps, we need to ensure we always try to match the longest
        // prefix first, as it's more specific.
        let prefix_map: Vec<(NonRootMPath, PrefixAction)> = {
            let mut v: Vec<(NonRootMPath, PrefixAction)> = prefix_map.into_iter().collect();
            v.sort_unstable_by_key(|(mpath, _)| mpath.len());
            v.reverse();
            v
        };
        Ok(CrossRepoMover {
            prefix_map,
            default_action,
            reverse_mover: Some(reverse_mover),
        })
    }
}

impl Mover for CrossRepoMover {
    fn move_path(&self, source_path: &NonRootMPath) -> Result<Option<NonRootMPath>> {
        let path_and_prefix_action = self
            .prefix_map
            .iter()
            .filter_map(|(candidate_prefix, candidate_action)| {
                get_suffix_after(source_path, candidate_prefix)
                    .map(move |suffix_after| (suffix_after, candidate_action))
            })
            .map(|(suffix_after, candidate_action)| {
                (
                    get_path_action(suffix_after, candidate_action),
                    candidate_action,
                )
            })
            .next();
        let mapped_path = match path_and_prefix_action {
            None => match self.default_action.clone() {
                DefaultAction::PrependPrefix(prefix) => Some(prefix.join(source_path)),
                DefaultAction::Preserve => Some(source_path.clone()),
                DefaultAction::DoNotSync => None,
            },
            Some((result_path_action, orig_prefix_action)) => result_path_action
                .map(|path_action| match path_action {
                    PathAction::Change(path) => Some(path),
                    PathAction::DoNotSync => None,
                })
                .with_context(|| {
                    ErrorKind::PrefixActionFailure(orig_prefix_action.clone(), source_path.clone())
                })
                .map_err(Error::from)?,
        };
        if let (Some(mapped_path), Some(reverse_mover)) = (&mapped_path, &self.reverse_mover) {
            if reverse_mover.move_path(mapped_path)?.as_ref() != Some(source_path) {
                return Ok(None);
            }
        };
        Ok(mapped_path)
    }

    fn conflicts_with(&self, path: &NonRootMPath) -> Result<bool> {
        match &self.default_action {
            DefaultAction::PrependPrefix(prefix) => {
                if prefix.is_related_to(path) {
                    return Ok(true);
                }
            }
            DefaultAction::Preserve => {
                return Ok(true);
            }
            DefaultAction::DoNotSync => {}
        }

        for (prefix, action) in &self.prefix_map {
            if prefix.is_related_to(path) {
                match action {
                    PrefixAction::Change(_) | PrefixAction::RemovePrefix => {
                        return Ok(true);
                    }
                    PrefixAction::DoNotSync => {}
                }
            }
        }

        Ok(false)
    }
}

// Given a full sync config and a small repo id,
// split it into this repo the rest
fn get_small_repo_and_others_from_config(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<(&SmallRepoCommitSyncConfig, Vec<&SmallRepoCommitSyncConfig>)> {
    let small_repo = match &commit_sync_config.small_repos.get(&small_repo_id) {
        Some(config) => *config,
        None => return Err(Error::from(ErrorKind::SmallRepoNotFound(small_repo_id))),
    };
    let others: Vec<_> = commit_sync_config
        .small_repos
        .iter()
        .filter_map(|(k, v)| if k != &small_repo_id { Some(v) } else { None })
        .collect();
    Ok((small_repo, others))
}

/// Get a mover for small-to-large repo sync
pub fn get_small_to_large_mover(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<Arc<dyn Mover>> {
    let (source_repo_config, _) =
        get_small_repo_and_others_from_config(commit_sync_config, small_repo_id)?;
    let default_action = source_repo_config.default_action.clone();
    let prefix_map = source_repo_config.map.clone();

    let default_action = DefaultAction::from_default_small_repo_action(default_action);
    let prefix_map: HashMap<_, _> = prefix_map
        .into_iter()
        .map(|(k, v)| (k, PrefixAction::Change(v)))
        .collect();

    Ok(Arc::new(CrossRepoMover::new(prefix_map, default_action)?))
}

/// Get a mover for a large-to-small repo sync
pub fn get_large_to_small_mover(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: RepositoryId,
) -> Result<Arc<dyn Mover>> {
    let (target_repo_config, other_repo_configs) =
        get_small_repo_and_others_from_config(commit_sync_config, small_repo_id)?;

    let target_repo_right_sides: HashSet<_> = target_repo_config.map.values().collect();

    let other_repo_right_sides: Vec<&NonRootMPath> = other_repo_configs
        .iter()
        .flat_map(|small_repo_config| {
            small_repo_config
                .map
                .values()
                .filter(|v| !target_repo_right_sides.contains(v))
        })
        .collect();

    let other_repo_prepended_prefixes: Vec<&NonRootMPath> = other_repo_configs
        .iter()
        .filter_map(
            |small_repo_config| match &small_repo_config.default_action {
                DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp) => Some(mp),
                _ => None,
            },
        )
        .collect();

    // We reverse the direction of all path-to-path mappings
    let mut prefix_map: HashMap<NonRootMPath, PrefixAction> = target_repo_config
        .map
        .iter()
        .map(|(k, v)| (v.clone(), PrefixAction::Change(k.clone())))
        .collect();

    // Any path that synced exclusively from some other small repo,
    // needs to be not synced back to this small repo. "Exclusively" is
    // needed here, because it is possible that two small repos sync some
    // dir to the same location in a large repo (that dir is identical),
    // and in that case commits to this dir in a large repo need to
    // sync to *both* small repos.
    other_repo_right_sides
        .into_iter()
        .chain(other_repo_prepended_prefixes)
        .for_each(|v| {
            prefix_map.insert(v.clone(), PrefixAction::DoNotSync);
        });

    // If small-to-large default action was not `Preserve`, we should
    // not sync this path, as `PrependPrefix` needs to be represented
    // by an individual `RemovePrefix` action in the map
    let default_action = match &target_repo_config.default_action {
        DefaultSmallToLargeCommitSyncPathAction::Preserve => DefaultAction::Preserve,
        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => {
            prefix_map.insert(prefix.clone(), PrefixAction::RemovePrefix);
            DefaultAction::DoNotSync
        }
    };

    // We will build a mover from the prefix_map and default_action we've just
    // constructed. However it doesn't work correctly for all edge cases.
    //
    // In particular, there might be multiple large repo paths that remap to the same
    // small repo path.
    // Consider this config
    // default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted")),
    // map: hashmap! {
    //     mp("preserved") => mp("preserved"),
    //     mp("preserved/excluded") => mp("shifted/preserved/excluded"),
    // },
    //
    // Now if we try to use the large to small mover to remap path shifted/preserved/1.txt,
    // then it will be remapped to preserved/1.txt, but this is incorrect, since preserved/1.txt
    // from small repo maps to preserved/1.txt in large repo.
    // To fix this issue we do the following: we take the corresponding small_to_large mover and
    // use it for a reverse mover check.  After mapping the path, it will reback the result back.
    // If this path is equal to the original path then we consider that the mapping is correct.
    // Otherwise we return None (i.e. a path from large repo doesn't remap to a path from small repo).
    let small_to_large_mover = get_small_to_large_mover(commit_sync_config, small_repo_id)?;
    Ok(Arc::new(CrossRepoMover::new_with_reverse_mover_check(
        prefix_map,
        default_action,
        small_to_large_mover,
    )?))
}

/// Get a forward and a reverse `Mover`, stored in the `Movers` struct
pub fn get_movers(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: RepositoryId,
    direction: CommitSyncDirection,
) -> Result<Movers> {
    match direction {
        CommitSyncDirection::Backwards => Ok(Movers {
            mover: get_large_to_small_mover(commit_sync_config, small_repo_id)?,
            reverse_mover: get_small_to_large_mover(commit_sync_config, small_repo_id)?,
        }),
        CommitSyncDirection::Forward => Ok(Movers {
            mover: get_small_to_large_mover(commit_sync_config, small_repo_id)?,
            reverse_mover: get_large_to_small_mover(commit_sync_config, small_repo_id)?,
        }),
    }
}

#[cfg(test)]
mod test {
    use maplit::hashmap;
    use metaconfig_types::CommitSyncConfigVersion;
    use mononoke_macros::mononoke;

    use super::*;

    fn mp(s: &'static str) -> NonRootMPath {
        NonRootMPath::new(s).unwrap()
    }

    fn mpe(s: &'static [u8]) -> MPathElement {
        MPathElement::new(s.to_vec()).unwrap()
    }

    #[mononoke::test]
    fn test_get_suffix_after() {
        let foobar = mp("foo/bar");
        let foo = mp("foo");
        let bar = mp("bar");
        assert_eq!(get_suffix_after(&foobar, &bar), None);
        let r: Vec<&MPathElement> = get_suffix_after(&foobar, &foo).unwrap();
        assert_eq!(r, vec![&mpe(b"bar")]);
        let r: Vec<&MPathElement> = get_suffix_after(&foobar, &foobar).unwrap();
        assert!(r.is_empty());
    }

    #[mononoke::test]
    fn test_get_path_action() {
        let foo_el = [mpe(b"foo")];
        assert_eq!(
            get_path_action(foo_el.iter(), &PrefixAction::DoNotSync).unwrap(),
            PathAction::DoNotSync
        );
        assert_eq!(
            get_path_action(foo_el.iter(), &PrefixAction::RemovePrefix).unwrap(),
            PathAction::Change(mp("foo"))
        );
        assert_eq!(
            get_path_action(foo_el.iter(), &PrefixAction::Change(mp("bar"))).unwrap(),
            PathAction::Change(mp("bar/foo"))
        );
    }

    #[mononoke::test]
    fn test_non_prefix_free_mover() {
        let hm = hashmap! {
            mp("path/") => PrefixAction::Change(mp("shortest/renamed")),
            mp("path/which/is/longest") => PrefixAction::Change(mp("longest/renamed")),
            mp("path/which/") => PrefixAction::Change(mp("middle/renamed")),
        };
        let mover = CrossRepoMover::new(hm, DefaultAction::DoNotSync).unwrap();
        assert_eq!(
            mover.move_path(&mp("path/which/is/longest/1.txt")).unwrap(),
            Some(mp("longest/renamed/1.txt"))
        );
        assert_eq!(
            mover.move_path(&mp("path/1.txt")).unwrap(),
            Some(mp("shortest/renamed/1.txt"))
        );
        assert_eq!(
            mover.move_path(&mp("path/which/2.txt")).unwrap(),
            Some(mp("middle/renamed/2.txt"))
        );
        assert_eq!(
            mover.move_path(&mp("path/which/subdir/2.txt")).unwrap(),
            Some(mp("middle/renamed/subdir/2.txt"))
        );
        assert_eq!(
            mover.move_path(&mp("path/subdir/1.txt")).unwrap(),
            Some(mp("shortest/renamed/subdir/1.txt"))
        );
    }

    #[mononoke::test]
    fn test_mover() {
        let hm = hashmap! {
            mp("renameme") => PrefixAction::Change(mp("renamed")),
            mp("deleteme") => PrefixAction::DoNotSync,
            mp("shiftme") => PrefixAction::Change(mp("shifted/shiftme")),
            mp("removeme") => PrefixAction::RemovePrefix,
        };
        let mover = CrossRepoMover::new(hm.clone(), DefaultAction::DoNotSync).unwrap();
        assert_eq!(
            mover.move_path(&mp("renameme/wow")).unwrap(),
            Some(mp("renamed/wow"))
        );
        assert_eq!(mover.move_path(&mp("deleteme/wow")).unwrap(), None);
        assert_eq!(
            mover.move_path(&mp("shiftme/wow")).unwrap(),
            Some(mp("shifted/shiftme/wow"))
        );
        assert_eq!(mover.move_path(&mp("wow")).unwrap(), None);
        assert_eq!(
            mover.move_path(&mp("removeme/wow")).unwrap(),
            Some(mp("wow"))
        );
        assert!(mover.move_path(&mp("removeme")).is_err());

        let mover = CrossRepoMover::new(hm.clone(), DefaultAction::Preserve).unwrap();
        assert_eq!(mover.move_path(&mp("wow")).unwrap(), Some(mp("wow")));

        let mover = CrossRepoMover::new(
            hm,
            DefaultAction::PrependPrefix(NonRootMPath::new("dude").unwrap()),
        )
        .unwrap();
        assert_eq!(mover.move_path(&mp("wow")).unwrap(), Some(mp("dude/wow")));
    }

    /*
    Below, the following sync config is tested:
    Small repo 1:
        (unmatched paths stay as they are)
        default action: preserve
        (a single dir is preserved from repo2, so has to be shifted in repo 1)
        "preserved2" => "repo1-rest/preserved2"
    Small repo 2:
        (unmatched paths go into "shifted2" subdir of a large repo)
        default action: prepend prefix "shifted2"
        (a single dir is preserved from repo2)
        "preserved2" => "preserved2"
        (some paths are moved into a different location)
        "sub1" => "repo2-rest/sub1"
        "sub2" => "repo2-rest/sub2"
    Note that in this configuration, the small repos have non-overlapping
    images in the big repo.
    */

    fn get_small_repo_sync_config_1_non_ovelapping() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
            map: hashmap! {
                mp("preserved2") => mp("repo1-rest/preserved2"),
            },
            submodule_config: Default::default(),
        }
    }

    fn get_small_repo_sync_config_2_non_ovelapping() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted2")),
            map: hashmap! {
                mp("preserved2") => mp("preserved2"),
                mp("sub1") => mp("repo2-rest/sub1"),
                mp("sub2") => mp("repo2-rest/sub2"),
            },
            submodule_config: Default::default(),
        }
    }

    fn get_large_repo_sync_config_non_overlapping() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(3),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_1_non_ovelapping(),
                RepositoryId::new(2) => get_small_repo_sync_config_2_non_ovelapping(),
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        }
    }

    #[mononoke::test]
    fn test_get_small_to_large_mover_1_non_overlapping() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover = get_small_to_large_mover(&large_sync_config, RepositoryId::new(1)).unwrap();

        // `preserved2` is a directory, preserved from repo2, so changes to
        // it in repo1 it have the shifted
        let f = mp("preserved2/f");
        assert_eq!(
            mover.move_path(&f).unwrap(),
            Some(mp("repo1-rest/preserved2/f"))
        );
        let f = mp("preserved2/d/f");
        assert_eq!(
            mover.move_path(&f).unwrap(),
            Some(mp("repo1-rest/preserved2/d/f"))
        );
        // `sub1` is a directory, remapped in repo2, but in repo1 is has
        // to be preserved
        let f = mp("sub1/f");
        assert_eq!(mover.move_path(&f).unwrap(), Some(f.clone()));
        // this is just a random file, not mentioned in either repo's configs
        // should be preserved, as repo1 has default_action preserve
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(mover.move_path(&f).unwrap(), Some(f.clone()));
    }

    #[mononoke::test]
    fn test_get_small_to_large_mover_2_non_overlapping() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover = get_small_to_large_mover(&large_sync_config, RepositoryId::new(2)).unwrap();

        // `preserved2` is a directory, preserved from repo2
        let f = mp("preserved2/f");
        assert_eq!(mover.move_path(&f).unwrap(), Some(mp("preserved2/f")));
        let f = mp("preserved2/d/f");
        assert_eq!(mover.move_path(&f).unwrap(), Some(mp("preserved2/d/f")));
        // `sub1` is a directory, remapped in repo2
        let f = mp("sub1/f");
        assert_eq!(mover.move_path(&f).unwrap(), Some(mp("repo2-rest/sub1/f")));
        let f = mp("sub2/d/f");
        assert_eq!(
            mover.move_path(&f).unwrap(),
            Some(mp("repo2-rest/sub2/d/f"))
        );
        // this is just a random file, not mentioned in either repo's configs
        // should be shifted, as repo2 has default_action prepend prefix
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(
            mover.move_path(&f).unwrap(),
            Some(mp("shifted2/aeneas/was/a/lively/fellow"))
        );
    }

    #[mononoke::test]
    fn test_get_large_to_small_mover_non_overlapping_images() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover_1 = get_large_to_small_mover(&large_sync_config, RepositoryId::new(1)).unwrap();
        let mover_2 = get_large_to_small_mover(&large_sync_config, RepositoryId::new(2)).unwrap();

        // any changes to large repo's `preserved2` dir could only come
        // from repo 1
        let f = mp("preserved2/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), None);
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("preserved2/f")));
        // any changes to large repo's `sub1` dir could only come from repo 1
        let f = mp("sub1/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), Some(mp("sub1/f")));
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        // any changes to large repo's `repo1-rest/preserved2` dir could
        // only come from repo 1
        let f = mp("repo1-rest/preserved2/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), Some(mp("preserved2/f")));
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        // any changes to large repo's `repo2-rest/sub1` dir could
        // only come from repo 2
        let f = mp("repo2-rest/sub1/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), None);
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("sub1/f")));
        // any changes to large repo's `shifted2` dir could
        // only come from repo 2
        let f = mp("shifted2/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), None);
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("f")));

        // Neither of the dirs below are remappings of any existing dir.
        // Neither `repo1-rest`, nor `repo2-rest` is a default
        // prependable prefix.
        // Changes to these dirs could only be preserved from repo 1
        let f = mp("repo1-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("repo1-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        let f = mp("repo2-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("repo2-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);

        // There no correct way to behave when the file has the same
        // name as a prependable prefix. Generally we will prevent
        // introducing files like this in the first place, but let's
        // make sure the code does the right thing. Note that commit
        // containing changes to such file will succeed syncing to
        // a repo, which does not use this prefix. We may want to
        // change that too, but failing to sync to one of the small
        // repos should be a signal enough to us that this needs looking.
        let prefix_only = mp("shifted2");
        assert!(mover_2.move_path(&prefix_only).is_err());
        assert_eq!(mover_1.move_path(&prefix_only).unwrap(), None);
    }

    /*
    Below, the following sync config is tested:
    Small repo 1:
        (unmatched paths stay as they are)
        default action: preserve
        (a directory, which was preserved from repo2 is
        now preserved from both, i.e. it is identical)
        "preserved2" => "preserved2"
    Small repo 2:
        (unmatched paths go into "shifted2" subdir of a large repo)
        default action: prepend prefix "shifted2"
        (a single dir is preserved from repo2)
        "preserved2" => "preserved2"
        (some paths are moved into a different location)
        "sub1" => "repo2-rest/sub1"
        "sub2" => "repo2-rest/sub2"
    Note that in this configuration, the small repos have overlapping
    images in the big repo. Separate testing of small-to-large configs
    in this scenario is not needed, but the large-to-small sync is
    different in this case.
    */

    fn get_large_repo_sync_config_overlapping() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(3),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                    map: hashmap! {
                        mp("preserved2") => mp("preserved2"),
                    },
                    submodule_config: Default::default(),
                },
                RepositoryId::new(2) => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted2")),
                    map: hashmap! {
                        mp("preserved2") => mp("preserved2"),
                        mp("sub1") => mp("repo2-rest/sub1"),
                        mp("sub2") => mp("repo2-rest/sub2"),
                    },
                    submodule_config: Default::default(),
                },
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        }
    }

    #[mononoke::test]
    fn test_get_large_to_small_mover_overlapping_images() {
        let mover_1 = get_large_to_small_mover(
            &get_large_repo_sync_config_overlapping(),
            RepositoryId::new(1),
        )
        .unwrap();
        let mover_2 = get_large_to_small_mover(
            &get_large_repo_sync_config_overlapping(),
            RepositoryId::new(2),
        )
        .unwrap();
        // `preserved2` is an identical directory, we should replay changes
        // to it to both small repos
        let f = mp("preserved2/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), Some(mp("preserved2/f")));
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("preserved2/f")));
        // any changes to large repo's `sub1` dir could only come from repo 1
        let f = mp("sub1/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), Some(mp("sub1/f")));
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        // any changes to large repo's `repo1-rest/preserved2` dir could
        // only come from repo 1
        let f = mp("repo1-rest/preserved2/f");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("repo1-rest/preserved2/f"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        // any changes to large repo's `repo2-rest/sub1` dir could
        // only come from repo 2
        let f = mp("repo2-rest/sub1/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), None);
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("sub1/f")));
        // any changes to large repo's `shifted2` dir could
        // only come from repo 2
        let f = mp("shifted2/f");
        assert_eq!(mover_1.move_path(&f).unwrap(), None);
        assert_eq!(mover_2.move_path(&f).unwrap(), Some(mp("f")));

        // Neither of the dirs below are remappings of any existing dir.
        // Neither `repo1-rest`, nor `repo2-rest` is a default
        // prependable prefix.
        // Changes to these dirs could only be preserved from repo 1
        let f = mp("repo1-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("repo1-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        let f = mp("repo2-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("repo2-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1.move_path(&f).unwrap(),
            Some(mp("aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2.move_path(&f).unwrap(), None);

        // There no correct way to behave when the file has the same
        // name as a prependable prefix. Generally we will prevent
        // introducing files like this in the first place, but let's
        // make sure the code does the right thing. Note that commit
        // containing changes to such file will succeed syncing to
        // a repo, which does not use this prefix. We may want to
        // change that too, but failing to sync to one of the small
        // repos should be a signal enough to us that this needs looking.
        let prefix_only = mp("shifted2");
        assert!(mover_2.move_path(&prefix_only).is_err());
        assert_eq!(mover_1.move_path(&prefix_only).unwrap(), None);
    }

    fn get_small_repo_sync_config_non_prefix_free() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted2")),
            map: hashmap! {
                mp("preserved2") => mp("preserved2"),
                mp("sub1") => mp("repo2-rest/sub1"),
                mp("sub1/preserved") => mp("sub1/preserved"),
            },
            submodule_config: Default::default(),
        }
    }

    fn get_large_repo_sync_config_non_prefix_free() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(2),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_non_prefix_free(),
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        }
    }

    #[mononoke::test]
    fn test_get_large_to_small_mover_non_prefix_free() -> Result<()> {
        let mover = get_large_to_small_mover(
            &get_large_repo_sync_config_non_prefix_free(),
            RepositoryId::new(1),
        )?;

        // `preserved2` is an identical directory, we should replay changes
        // to it to both small repos
        let f = mp("preserved2/f");
        assert_eq!(mover.move_path(&f)?, Some(mp("preserved2/f")));

        // This file is not from small repo, so should be remapped to None
        let f = mp("randomefile");
        assert_eq!(mover.move_path(&f)?, None);

        // Any changes to large repo's `sub1` dir could only come from repo 1
        let f = mp("repo2-rest/sub1/f");
        assert_eq!(mover.move_path(&f)?, Some(mp("sub1/f")));

        // This is an subtree of sub1, but this subtree is preserved. Make
        // sure path doesn't change
        let f = mp("sub1/preserved");
        assert_eq!(mover.move_path(&f)?, Some(mp("sub1/preserved")));

        Ok(())
    }

    fn get_small_repo_sync_config_with_excludes() -> SmallRepoCommitSyncConfig {
        SmallRepoCommitSyncConfig {
            default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted")),
            map: hashmap! {
                mp("preserved") => mp("preserved"),
                mp("preserved/excluded") => mp("shifted/preserved/excluded"),
            },
            submodule_config: Default::default(),
        }
    }

    fn get_large_repo_sync_config_with_exludes() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: RepositoryId::new(2),
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                RepositoryId::new(1) => get_small_repo_sync_config_with_excludes(),
            },
            version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
        }
    }

    #[mononoke::test]
    fn test_get_large_to_small_mover_with_excludes() -> Result<()> {
        let mover = get_large_to_small_mover(
            &get_large_repo_sync_config_with_exludes(),
            RepositoryId::new(1),
        )?;

        let f = mp("shifted/f");
        assert_eq!(mover.move_path(&f)?, Some(mp("f")));

        let f = mp("shifted/preserved/1.txt");
        assert_eq!(mover.move_path(&f)?, None);

        let f = mp("shifted/preserved/excluded/1");
        assert_eq!(mover.move_path(&f)?, Some(mp("preserved/excluded/1")));

        let f = mp("preserved/excluded/1");
        assert_eq!(mover.move_path(&f)?, None);

        Ok(())
    }
}
