// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::{prelude::*, Error};
use itertools::Itertools;
use mercurial_types::{MPath, MPathElement};
use metaconfig_types::{
    CommitSyncConfig, DefaultSmallToLargeCommitSyncPathAction, SmallRepoCommitSyncConfig,
};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::iter::Iterator;
use std::sync::Arc;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Cannot remove prefix, equal to the whole path")]
    RemovePrefixWholePathFailure,
    #[fail(display = "Cannot apply prefix action {:?} to {:?}", _0, _1)]
    PrefixActionFailure(PrefixAction, MPath),
    #[fail(display = "Small repo {} not found", _0)]
    SmallRepoNotFound(i32),
    #[fail(
        display = "Provided map is not prefix-free (e.g. {:?} and {:?})",
        _0, _1
    )]
    NonPrefixFreeMap(MPath, MPath),
}

/// A function to modify paths during repo sync
/// Here are the meanings of the return values:
/// - `Ok(Some(newpath))` - the path should be
///   replaced with `newpath` during sync
/// - `Ok(None)` - the path shoould not be synced
/// - `Err(e)` - the sync should fail, as this function
///   could not figure out how to rewrite path
pub type Mover = Arc<dyn Fn(&MPath) -> Result<Option<MPath>> + Send + Sync + 'static>;

/// An action, configured for a given prefix
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrefixAction {
    // The new path should have this prefix replaced with a new value
    Change(MPath),
    // The new path should have this prefix dropped
    RemovePrefix,
    // The path that matches this prefix should not be synced
    DoNotSync,
}

/// An action, applied to the entire path
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathAction {
    // Change the path when syncing
    Change(MPath),
    // Do not sync this path
    DoNotSync,
}

/// Default action to apply to a path when syncing between two repos
#[derive(Debug, Clone)]
enum DefaultAction {
    /// Prepend path with this prefix
    PrependPrefix(MPath),
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
    source_path: &'a MPath,
    candidate_prefix: &'b MPath,
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
            MPath::try_from(elements)
                .map(|mpath| PathAction::Change(mpath))
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

/// Check if no element of this vector is a prefix of another element
fn fail_if_not_prefix_free<'a>(paths: Vec<&'a MPath>) -> Result<()> {
    let r: Result<Vec<_>> = paths
        .into_iter()
        .tuple_combinations::<(_, _)>()
        .map(|(p1, p2)| {
            if p1.is_prefix_of(p2) || p2.is_prefix_of(p1) {
                Err(Error::from(ErrorKind::NonPrefixFreeMap(
                    p1.clone(),
                    p2.clone(),
                )))
            } else {
                Ok(())
            }
        })
        .collect();
    r.map(|_| ())
}

/// Create a `Mover`, given a path prefix map and a default action
fn mover_factory(
    prefix_map: HashMap<MPath, PrefixAction>,
    default_action: DefaultAction,
) -> Result<Mover> {
    let keys: Vec<&MPath> = prefix_map.iter().map(|(k, _)| k).collect();
    fail_if_not_prefix_free(keys)?;

    Ok(Arc::new(move |source_path: &MPath| {
        let path_and_prefix_action = prefix_map
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
            .nth(0);
        match path_and_prefix_action {
            None => Ok(match default_action.clone() {
                DefaultAction::PrependPrefix(prefix) => Some(prefix.join(source_path.into_iter())),
                DefaultAction::Preserve => Some(source_path.clone()),
                DefaultAction::DoNotSync => None,
            }),
            Some((result_path_action, orig_prefix_action)) => result_path_action
                .map(|path_action| match path_action {
                    PathAction::Change(path) => Some(path.clone()),
                    PathAction::DoNotSync => None,
                })
                .chain_err(ErrorKind::PrefixActionFailure(
                    orig_prefix_action.clone(),
                    source_path.clone(),
                ))
                .map_err(Error::from),
        }
    }))
}

// Given a full sync config and a small repo id,
// split it into this repo the rest
fn get_small_repo_and_others_from_config(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: i32,
) -> Result<(&SmallRepoCommitSyncConfig, Vec<&SmallRepoCommitSyncConfig>)> {
    let small_repo = match &commit_sync_config.small_repos.get(&small_repo_id) {
        Some(config) => config.clone(),
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
    small_repo_id: i32,
) -> Result<Mover> {
    let (source_repo_config, _) =
        get_small_repo_and_others_from_config(commit_sync_config, small_repo_id)?;
    let default_action = source_repo_config.default_action.clone();
    let prefix_map = source_repo_config.map.clone();

    let default_action = DefaultAction::from_default_small_repo_action(default_action);
    let prefix_map: HashMap<_, _> = prefix_map
        .into_iter()
        .map(|(k, v)| (k, PrefixAction::Change(v)))
        .collect();
    mover_factory(prefix_map, default_action)
}

/// Get a mover for a large-to-small repo sync
pub fn get_large_to_small_mover(
    commit_sync_config: &CommitSyncConfig,
    small_repo_id: i32,
) -> Result<Mover> {
    let (target_repo_config, other_repo_configs) =
        get_small_repo_and_others_from_config(commit_sync_config, small_repo_id)?;

    let target_repo_right_sides: HashSet<_> = target_repo_config.map.values().collect();

    let other_repo_right_sides: Vec<&MPath> = other_repo_configs
        .iter()
        .map(|small_repo_config| {
            small_repo_config
                .map
                .values()
                .filter(|v| !target_repo_right_sides.contains(v))
        })
        .flatten()
        .collect();

    let other_repo_prepended_prefixes: Vec<&MPath> = other_repo_configs
        .iter()
        .filter_map(
            |small_repo_config| match &small_repo_config.default_action {
                DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp) => Some(mp),
                _ => None,
            },
        )
        .collect();

    // We reverse the direction of all path-to-path mappings
    let mut prefix_map: HashMap<MPath, PrefixAction> = target_repo_config
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
        .chain(other_repo_prepended_prefixes.into_iter())
        .for_each(|v| {
            prefix_map.insert(v.clone(), PrefixAction::DoNotSync);
        });

    // If small-to-large default action was not `Preserve`, we should
    // not sycn this path, as `PrependPrefix` needs to be represented
    // by an individual `RemovePrefix` action in the map
    let default_action = match &target_repo_config.default_action {
        DefaultSmallToLargeCommitSyncPathAction::Preserve => DefaultAction::Preserve,
        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => {
            prefix_map.insert(prefix.clone(), PrefixAction::RemovePrefix);
            DefaultAction::DoNotSync
        }
    };
    mover_factory(prefix_map, default_action)
}

#[cfg(test)]
mod test {
    use super::*;
    use ascii::AsciiString;
    use maplit::hashmap;
    use metaconfig_types::CommitSyncDirection;

    fn mp(s: &'static str) -> MPath {
        MPath::new(s).unwrap()
    }

    fn mpe(s: &'static [u8]) -> MPathElement {
        MPathElement::new(s.to_vec()).unwrap()
    }

    #[test]
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

    #[test]
    fn test_get_path_action() {
        let foo_el = vec![mpe(b"foo")];
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

    #[test]
    fn test_mover() {
        let hm = hashmap! {
            mp("renameme") => PrefixAction::Change(mp("renamed")),
            mp("deleteme") => PrefixAction::DoNotSync,
            mp("shiftme") => PrefixAction::Change(mp("shifted/shiftme")),
            mp("removeme") => PrefixAction::RemovePrefix,
        };
        let mover = mover_factory(hm.clone(), DefaultAction::DoNotSync).unwrap();
        assert_eq!(mover(&mp("renameme/wow")).unwrap(), Some(mp("renamed/wow")));
        assert_eq!(mover(&mp("deleteme/wow")).unwrap(), None);
        assert_eq!(
            mover(&mp("shiftme/wow")).unwrap(),
            Some(mp("shifted/shiftme/wow"))
        );
        assert_eq!(mover(&mp("wow")).unwrap(), None);
        assert_eq!(mover(&mp("removeme/wow")).unwrap(), Some(mp("wow")));
        assert!(mover(&mp("removeme")).is_err());

        let mover = mover_factory(hm.clone(), DefaultAction::Preserve).unwrap();
        assert_eq!(mover(&mp("wow")).unwrap(), Some(mp("wow")));

        let mover = mover_factory(
            hm.clone(),
            DefaultAction::PrependPrefix(MPath::new("dude").unwrap()),
        )
        .unwrap();
        assert_eq!(mover(&mp("wow")).unwrap(), Some(mp("dude/wow")));
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
            bookmark_prefix: AsciiString::from_ascii("b1".to_string()).unwrap(),
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
            bookmark_prefix: AsciiString::from_ascii("b2".to_string()).unwrap(),
        }
    }

    fn get_large_repo_sync_config_non_overlapping() -> CommitSyncConfig {
        CommitSyncConfig {
            large_repo_id: 3,
            direction: CommitSyncDirection::LargeToSmall,
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                1 => get_small_repo_sync_config_1_non_ovelapping(),
                2 => get_small_repo_sync_config_2_non_ovelapping(),
            },
        }
    }

    #[test]
    fn test_get_small_to_large_mover_1_non_overlapping() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover = get_small_to_large_mover(&large_sync_config, 1).unwrap();

        // `preserved2` is a directory, preserved from repo2, so changes to
        // it in repo1 it have tbe shifted
        let f = mp("preserved2/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("repo1-rest/preserved2/f")));
        let f = mp("preserved2/d/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("repo1-rest/preserved2/d/f")));
        // `sub1` is a directory, remapped in repo2, but in repo1 is has
        // to be preserved
        let f = mp("sub1/f");
        assert_eq!(mover(&f).unwrap(), Some(f.clone()));
        // this is just a random file, not mentioned in either repo's configs
        // should be preserved, as repo1 has default_action preserve
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(mover(&f).unwrap(), Some(f.clone()));
    }

    #[test]
    fn test_get_small_to_large_mover_2_non_overlapping() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover = get_small_to_large_mover(&large_sync_config, 2).unwrap();

        // `preserved2` is a directory, preserved from repo2
        let f = mp("preserved2/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("preserved2/f")));
        let f = mp("preserved2/d/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("preserved2/d/f")));
        // `sub1` is a directory, remapped in repo2
        let f = mp("sub1/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("repo2-rest/sub1/f")));
        let f = mp("sub2/d/f");
        assert_eq!(mover(&f).unwrap(), Some(mp("repo2-rest/sub2/d/f")));
        // this is just a random file, not mentioned in either repo's configs
        // should be shifted, as repo2 has default_action prepend prefix
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(
            mover(&f).unwrap(),
            Some(mp("shifted2/aeneas/was/a/lively/fellow"))
        );
    }

    #[test]
    fn test_get_large_to_small_mover_non_overlapping_images() {
        let large_sync_config = get_large_repo_sync_config_non_overlapping();
        let mover_1 = get_large_to_small_mover(&large_sync_config, 1).unwrap();
        let mover_2 = get_large_to_small_mover(&large_sync_config, 2).unwrap();

        // any changes to large repo's `preserved2` dir could only come
        // from repo 1
        let f = mp("preserved2/f");
        assert_eq!(mover_1(&f).unwrap(), None);
        assert_eq!(mover_2(&f).unwrap(), Some(mp("preserved2/f")));
        // any changes to large repo's `sub1` dir could only come from repo 1
        let f = mp("sub1/f");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("sub1/f")));
        assert_eq!(mover_2(&f).unwrap(), None);
        // any changes to large repo's `repo1-rest/preserved2` dir could
        // only come from repo 1
        let f = mp("repo1-rest/preserved2/f");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("preserved2/f")));
        assert_eq!(mover_2(&f).unwrap(), None);
        // any changes to large repo's `repo2-rest/sub1` dir could
        // only come from repo 2
        let f = mp("repo2-rest/sub1/f");
        assert_eq!(mover_1(&f).unwrap(), None);
        assert_eq!(mover_2(&f).unwrap(), Some(mp("sub1/f")));
        // any changes to large repo's `shifted2` dir could
        // only come from repo 2
        let f = mp("shifted2/f");
        assert_eq!(mover_1(&f).unwrap(), None);
        assert_eq!(mover_2(&f).unwrap(), Some(mp("f")));

        // Neither of the dirs below are remappings of any existing dir.
        // Neither `repo1-rest`, nor `repo2-rest` is a default
        // prependable prefix.
        // Changes to these dirs could only be preserved from repo 1
        let f = mp("repo1-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1(&f).unwrap(),
            Some(mp("repo1-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2(&f).unwrap(), None);
        let f = mp("repo2-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1(&f).unwrap(),
            Some(mp("repo2-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2(&f).unwrap(), None);
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("aeneas/was/a/lively/fellow")));
        assert_eq!(mover_2(&f).unwrap(), None);

        // There no correct way to behave when the file has the same
        // name as a prependable prefix. Generally we will prevent
        // introducting files like this in the first place, but let's
        // make sure the code does the right thing. Note that commit
        // containing changes to such file will succeed syncing to
        // a repo, which does not use this prefix. We may want to
        // change that too, but failing to sync to one of the small
        // repos should be a signal enough to us that this needs looking.
        let prefix_only = mp("shifted2");
        assert!(mover_2(&prefix_only).is_err());
        assert_eq!(mover_1(&prefix_only).unwrap(), None);
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
            large_repo_id: 3,
            direction: CommitSyncDirection::LargeToSmall,
            common_pushrebase_bookmarks: vec![],
            small_repos: hashmap! {
                1 => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                    map: hashmap! {
                        mp("preserved2") => mp("preserved2"),
                    },
                    bookmark_prefix: AsciiString::from_ascii("b1".to_string()).unwrap(),
                },
                2 => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(mp("shifted2")),
                    map: hashmap! {
                        mp("preserved2") => mp("preserved2"),
                        mp("sub1") => mp("repo2-rest/sub1"),
                        mp("sub2") => mp("repo2-rest/sub2"),
                    },
                    bookmark_prefix: AsciiString::from_ascii("b2".to_string()).unwrap(),
                },
            },
        }
    }

    #[test]
    fn test_get_large_to_small_mover_overlapping_images() {
        let mover_1 =
            get_large_to_small_mover(&get_large_repo_sync_config_overlapping(), 1).unwrap();
        let mover_2 =
            get_large_to_small_mover(&get_large_repo_sync_config_overlapping(), 2).unwrap();
        // `preserved2` is an identical directory, we should replay changes
        // to it to both small repos
        let f = mp("preserved2/f");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("preserved2/f")));
        assert_eq!(mover_2(&f).unwrap(), Some(mp("preserved2/f")));
        // any changes to large repo's `sub1` dir could only come from repo 1
        let f = mp("sub1/f");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("sub1/f")));
        assert_eq!(mover_2(&f).unwrap(), None);
        // any changes to large repo's `repo1-rest/preserved2` dir could
        // only come from repo 1
        let f = mp("repo1-rest/preserved2/f");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("repo1-rest/preserved2/f")));
        assert_eq!(mover_2(&f).unwrap(), None);
        // any changes to large repo's `repo2-rest/sub1` dir could
        // only come from repo 2
        let f = mp("repo2-rest/sub1/f");
        assert_eq!(mover_1(&f).unwrap(), None);
        assert_eq!(mover_2(&f).unwrap(), Some(mp("sub1/f")));
        // any changes to large repo's `shifted2` dir could
        // only come from repo 2
        let f = mp("shifted2/f");
        assert_eq!(mover_1(&f).unwrap(), None);
        assert_eq!(mover_2(&f).unwrap(), Some(mp("f")));

        // Neither of the dirs below are remappings of any existing dir.
        // Neither `repo1-rest`, nor `repo2-rest` is a default
        // prependable prefix.
        // Changes to these dirs could only be preserved from repo 1
        let f = mp("repo1-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1(&f).unwrap(),
            Some(mp("repo1-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2(&f).unwrap(), None);
        let f = mp("repo2-rest/aeneas/was/a/lively/fellow");
        assert_eq!(
            mover_1(&f).unwrap(),
            Some(mp("repo2-rest/aeneas/was/a/lively/fellow"))
        );
        assert_eq!(mover_2(&f).unwrap(), None);
        let f = mp("aeneas/was/a/lively/fellow");
        assert_eq!(mover_1(&f).unwrap(), Some(mp("aeneas/was/a/lively/fellow")));
        assert_eq!(mover_2(&f).unwrap(), None);

        // There no correct way to behave when the file has the same
        // name as a prependable prefix. Generally we will prevent
        // introducting files like this in the first place, but let's
        // make sure the code does the right thing. Note that commit
        // containing changes to such file will succeed syncing to
        // a repo, which does not use this prefix. We may want to
        // change that too, but failing to sync to one of the small
        // repos should be a signal enough to us that this needs looking.
        let prefix_only = mp("shifted2");
        assert!(mover_2(&prefix_only).is_err());
        assert_eq!(mover_1(&prefix_only).unwrap(), None);
    }

}
