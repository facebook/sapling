// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type, try_from)]

extern crate async_unit;
extern crate blobrepo;
extern crate fixtures;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate maplit;
extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate tokio;

use std::collections::{BTreeMap, HashSet};
use std::iter::repeat;
use std::str::FromStr;
use std::sync::Arc;

use blobrepo::BlobRepo;
use fixtures::many_files_dirs;
use futures::{Future, Stream};
use futures::executor::spawn;
use futures_ext::select_all;
use mercurial_types::{Changeset, Entry, FileType, MPath, MPathElement, Manifest, RepoPath, Type,
                      NULL_HASH};
use mercurial_types::manifest::{Content, EmptyManifest};
use mercurial_types::manifest_utils::{and_pruner_combinator, changed_entry_stream,
                                      changed_entry_stream_with_pruner, diff_sorted_vecs,
                                      file_pruner, recursive_entry_stream, visited_pruner,
                                      ChangedEntry, EntryStatus};
use mercurial_types::nodehash::{HgChangesetId, HgEntryId, HgNodeHash};
use mercurial_types_mocks::manifest::{ContentFactory, MockEntry, MockManifest};
use mercurial_types_mocks::nodehash;

fn get_root_manifest(repo: Arc<BlobRepo>, changesetid: &HgChangesetId) -> Box<Manifest> {
    let cs = repo.get_changeset_by_changesetid(changesetid)
        .wait()
        .unwrap();
    let manifestid = cs.manifestid();
    repo.get_manifest_by_nodeid(&manifestid.into_nodehash())
        .wait()
        .unwrap()
}

fn get_hash(c: char) -> HgEntryId {
    let hash: String = repeat(c).take(40).collect();
    HgEntryId::new(HgNodeHash::from_str(&hash).unwrap())
}

fn get_entry(ty: Type, hash: HgEntryId, path: RepoPath) -> Box<Entry + Sync> {
    let content_factory: ContentFactory = Arc::new(|| -> Content {
        panic!("should not be called");
    });

    let mut entry = MockEntry::new(path, content_factory);
    entry.set_type(ty);
    entry.set_hash(hash);
    Box::new(entry)
}

fn count_entries(entries: &Vec<ChangedEntry>) -> (usize, usize, usize) {
    let mut added = 0;
    let mut modified = 0;
    let mut deleted = 0;

    for entry in entries {
        match entry.status {
            EntryStatus::Added(..) => {
                added += 1;
            }
            EntryStatus::Modified { .. } => modified += 1,
            EntryStatus::Deleted(..) => {
                deleted += 1;
            }
        }
    }

    return (added, modified, deleted);
}

#[test]
fn test_diff_sorted_vecs_simple() {
    let path = RepoPath::file("file.txt").unwrap();

    let left_entry = get_entry(Type::File(FileType::Regular), get_hash('1'), path.clone());
    let right_entry = get_entry(Type::File(FileType::Regular), get_hash('2'), path.clone());
    let res = diff_sorted_vecs(None, vec![left_entry], vec![right_entry]);

    assert_eq!(res.len(), 1);
    let (_, modified, _) = count_entries(&res);
    assert_eq!(modified, 1);

    // With different types we should get added and deleted entries
    let left_entry = get_entry(Type::File(FileType::Regular), get_hash('1'), path.clone());
    let right_entry = get_entry(Type::Tree, get_hash('2'), path.clone());
    let res = diff_sorted_vecs(None, vec![left_entry], vec![right_entry]);

    assert_eq!(res.len(), 2);
    let (added, _, deleted) = count_entries(&res);
    assert_eq!(added, 1);
    assert_eq!(deleted, 1);
}

#[test]
fn test_diff_sorted_vecs_added_deleted() {
    let left_path = RepoPath::file("file1.txt").unwrap();
    let right_path = RepoPath::file("file2.txt").unwrap();

    let left_entry = get_entry(Type::File(FileType::Regular), get_hash('1'), left_path);
    let right_entry = get_entry(Type::File(FileType::Regular), get_hash('2'), right_path);
    let res = diff_sorted_vecs(None, vec![left_entry], vec![right_entry]);

    assert_eq!(res.len(), 2);
    let (added, _, deleted) = count_entries(&res);
    assert_eq!(added, 1);
    assert_eq!(deleted, 1);
}

#[test]
fn test_diff_sorted_vecs_one_added_one_same() {
    {
        let left_path_first = RepoPath::file("a.txt").unwrap();
        let path_second = RepoPath::file("file.txt").unwrap();

        let left_entry_first = get_entry(
            Type::File(FileType::Regular),
            get_hash('1'),
            left_path_first,
        );
        let left_entry_second = get_entry(
            Type::File(FileType::Regular),
            get_hash('2'),
            path_second.clone(),
        );
        let right_entry = get_entry(Type::File(FileType::Regular), get_hash('2'), path_second);

        let res = diff_sorted_vecs(
            None,
            vec![left_entry_first, left_entry_second],
            vec![right_entry],
        );

        assert_eq!(res.len(), 1);
        let (added, ..) = count_entries(&res);
        assert_eq!(added, 1);
    }

    // Now change the order: left has one file that has a 'bigger' filename
    {
        let path_first = RepoPath::file("file.txt").unwrap();
        let left_path_second = RepoPath::file("z.txt").unwrap();

        let left_entry_first = get_entry(
            Type::File(FileType::Regular),
            get_hash('1'),
            path_first.clone(),
        );
        let left_entry_second = get_entry(
            Type::File(FileType::Regular),
            get_hash('2'),
            left_path_second,
        );
        let right_entry = get_entry(Type::File(FileType::Regular), get_hash('1'), path_first);

        let res = diff_sorted_vecs(
            None,
            vec![left_entry_first, left_entry_second],
            vec![right_entry],
        );

        assert_eq!(res.len(), 1);
        let (added, ..) = count_entries(&res);
        assert_eq!(added, 1);
    }
}

#[test]
fn test_diff_sorted_vecs_one_empty() {
    let path = RepoPath::file("file.txt").unwrap();

    let entry = get_entry(Type::File(FileType::Regular), get_hash('1'), path);
    let res = diff_sorted_vecs(None, vec![entry], vec![]);

    assert_eq!(res.len(), 1);
    let (added, ..) = count_entries(&res);
    assert_eq!(added, 1);
}

fn find_changed_entry_status_stream(
    manifest: Box<Manifest>,
    basemanifest: Box<Manifest>,
    pruner: impl FnMut(&ChangedEntry) -> bool + Send + Clone + 'static,
    max_depth: Option<usize>,
) -> Vec<ChangedEntry> {
    let mut stream = spawn(changed_entry_stream_with_pruner(
        &manifest,
        &basemanifest,
        None,
        pruner,
        max_depth,
    ));
    let mut res = vec![];
    loop {
        let new_elem = stream.wait_stream();
        match new_elem {
            Some(elem) => {
                let elem = elem.expect("Unexpected error");
                res.push(elem);
            }
            None => {
                break;
            }
        }
    }
    res
}

fn check_changed_paths(
    actual: Vec<ChangedEntry>,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
) {
    let mut paths_added = vec![];
    let mut paths_deleted = vec![];
    let mut paths_modified = vec![];

    for changed_entry in actual {
        match changed_entry.status {
            EntryStatus::Added(entry) => {
                paths_added.push(MPath::join_element_opt(
                    changed_entry.dirname.as_ref(),
                    entry.get_name(),
                ));
            }
            EntryStatus::Deleted(entry) => {
                paths_deleted.push(MPath::join_element_opt(
                    changed_entry.dirname.as_ref(),
                    entry.get_name(),
                ));
            }
            EntryStatus::Modified {
                to_entry,
                from_entry,
            } => {
                assert_eq!(to_entry.get_type(), from_entry.get_type());
                paths_modified.push(MPath::join_element_opt(
                    changed_entry.dirname.as_ref(),
                    to_entry.get_name(),
                ));
            }
        }
    }

    fn compare(change_name: &str, actual: Vec<Option<MPath>>, expected: Vec<&str>) {
        let actual_set: HashSet<_> = actual
            .iter()
            .map(|path| match *path {
                Some(ref path) => path.to_vec(),
                None => vec![],
            })
            .collect();
        let expected_set: HashSet<_> = expected
            .iter()
            .map(|s| (*s).to_owned().into_bytes())
            .collect();

        assert_eq!(
            actual_set, expected_set,
            "{} check failed! expected: {:?}, got: {:?}",
            change_name, expected, actual,
        );
    }

    compare("added", paths_added, expected_added);
    compare("deleted", paths_deleted, expected_deleted);
    compare("modified", paths_modified, expected_modified);
}

fn do_check_with_pruner<P>(
    repo: Arc<BlobRepo>,
    main_hash: HgNodeHash,
    base_hash: HgNodeHash,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
    pruner: P,
    max_depth: Option<usize>,
) where
    P: FnMut(&ChangedEntry) -> bool + Send + Clone + 'static,
{
    {
        let manifest = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash));
        let base_manifest = get_root_manifest(repo.clone(), &HgChangesetId::new(base_hash));

        let res =
            find_changed_entry_status_stream(manifest, base_manifest, pruner.clone(), max_depth);

        check_changed_paths(
            res,
            expected_added.clone(),
            expected_deleted.clone(),
            expected_modified.clone(),
        );
    }

    // Vice-versa: compare base_hash to main_hash. Deleted paths become added, added become
    // deleted.
    {
        let manifest = get_root_manifest(repo.clone(), &HgChangesetId::new(base_hash));
        let base_manifest = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash));

        let res = find_changed_entry_status_stream(manifest, base_manifest, pruner, max_depth);

        check_changed_paths(
            res,
            expected_deleted.clone(),
            expected_added.clone(),
            expected_modified.clone(),
        );
    }
}

fn do_check(
    repo: Arc<BlobRepo>,
    main_hash: HgNodeHash,
    base_hash: HgNodeHash,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
) {
    do_check_with_pruner(
        repo,
        main_hash,
        base_hash,
        expected_added,
        expected_deleted,
        expected_modified,
        |_| true,
        None,
    )
}

#[test]
fn test_recursive_changed_entry_stream_simple() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        let base_hash = HgNodeHash::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8").unwrap();
        // main_hash is a child of base_hash
        // hg st --change .
        // A 2
        // A dir1/file_1_in_dir1
        // A dir1/file_2_in_dir1
        // A dir1/subdir1/file_1
        // A dir2/file_1_in_dir2

        // 8 entries were added: top-level dirs 'dir1' and 'dir2' and file 'A',
        // two files 'file_1_in_dir1' and 'file_2_in_dir1' and dir 'subdir1' inside 'dir1'
        // 'file_1_in_dir2' inside dir2 and 'file_1' inside 'dir1/subdir1/file_1'

        let expected_added = vec![
            "2",
            "dir1",
            "dir1/file_1_in_dir1",
            "dir1/file_2_in_dir1",
            "dir1/subdir1",
            "dir1/subdir1/file_1",
            "dir2",
            "dir2/file_1_in_dir2",
        ];
        do_check(repo, main_hash, base_hash, expected_added, vec![], vec![]);
        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_entry_stream() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let changesetid = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();

        // hg up 2f866e7e549760934e31bf0420a873f65100ad63
        // $ hg files
        // 1
        // 2
        // dir1/file_1_in_dir1
        // dir1/file_2_in_dir1
        // dir1/subdir1/file_1
        // dir2/file_1_in_dir2

        let cs = repo.get_changeset_by_changesetid(&HgChangesetId::new(changesetid))
            .wait()
            .unwrap();
        let manifestid = cs.manifestid();

        let root_entry = repo.get_root_entry(&manifestid);
        let fut = recursive_entry_stream(None, root_entry).collect();
        let res = fut.wait().unwrap();

        let mut actual = hashset![];
        for r in res {
            let path = MPath::join_element_opt(r.0.as_ref(), r.1.get_name());
            actual.insert(path);
        }
        let expected = hashset![
            None,
            Some(MPath::new("1").unwrap()),
            Some(MPath::new("2").unwrap()),
            Some(MPath::new("dir1").unwrap()),
            Some(MPath::new("dir1/file_1_in_dir1").unwrap()),
            Some(MPath::new("dir1/file_2_in_dir1").unwrap()),
            Some(MPath::new("dir1/subdir1").unwrap()),
            Some(MPath::new("dir1/subdir1/file_1").unwrap()),
            Some(MPath::new("dir2").unwrap()),
            Some(MPath::new("dir2/file_1_in_dir2").unwrap()),
        ];

        assert_eq!(actual, expected);

        let root_mf = repo.get_manifest_by_nodeid(&manifestid.into_nodehash())
            .wait()
            .unwrap();

        let path_element = MPathElement::new(Vec::from("dir1".as_bytes())).unwrap();
        let subentry = root_mf.lookup(&path_element).unwrap();

        let res = recursive_entry_stream(None, subentry)
            .collect()
            .wait()
            .unwrap();
        let mut actual = hashset![];
        for r in res {
            let path = MPath::join_element_opt(r.0.as_ref(), r.1.get_name());
            actual.insert(path);
        }
        let expected = hashset![
            Some(MPath::new("dir1").unwrap()),
            Some(MPath::new("dir1/file_1_in_dir1").unwrap()),
            Some(MPath::new("dir1/file_2_in_dir1").unwrap()),
            Some(MPath::new("dir1/subdir1").unwrap()),
            Some(MPath::new("dir1/subdir1/file_1").unwrap()),
        ];

        assert_eq!(actual, expected);

        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_changed_entry_stream_changed_dirs() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        let base_hash = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        // main_hash is a child of base_hash
        // hg st --change .
        // A dir1/subdir1/subsubdir1/file_1
        // A dir1/subdir1/subsubdir2/file_1
        // A dir1/subdir1/subsubdir2/file_2
        let expected_added = vec![
            "dir1/subdir1/subsubdir1",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2",
            "dir1/subdir1/subsubdir2/file_1",
            "dir1/subdir1/subsubdir2/file_2",
        ];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check(
            repo,
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
        );
        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_changed_entry_stream_dirs_replaced_with_file() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash = HgNodeHash::from_str("0c59c8d0da93cbf9d7f4b888f28823ffb2e3e480").unwrap();
        let base_hash = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        // main_hash is a child of base_hash
        // hg st --change .
        // A dir1
        // R dir1/file_1_in_dir1
        // R dir1/file_2_in_dir1
        // R dir1/subdir1/file_1
        // R dir1/subdir1/subsubdir1/file_1
        // R dir1/subdir1/subsubdir2/file_1
        // R dir1/subdir1/subsubdir2/file_2

        let expected_added = vec!["dir1"];
        let expected_deleted = vec![
            "dir1",
            "dir1/file_1_in_dir1",
            "dir1/file_2_in_dir1",
            "dir1/subdir1",
            "dir1/subdir1/file_1",
            "dir1/subdir1/subsubdir1",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2",
            "dir1/subdir1/subsubdir2/file_1",
            "dir1/subdir1/subsubdir2/file_2",
        ];
        do_check(
            repo,
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
        );
        Ok(())
    }).expect("test failed")
}

#[test]
fn test_depth_parameter() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        let base_hash = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        // main_hash is a child of base_hash
        // hg st --change .
        // A dir1/subdir1/subsubdir1/file_1
        // A dir1/subdir1/subsubdir2/file_1
        // A dir1/subdir1/subsubdir2/file_2
        let expected_added = vec![
            "dir1/subdir1/subsubdir1",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2",
            "dir1/subdir1/subsubdir2/file_1",
            "dir1/subdir1/subsubdir2/file_2",
        ];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            |_| true,
            Some(4),
        );

        let expected_added = vec!["dir1/subdir1/subsubdir1", "dir1/subdir1/subsubdir2"];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            |_| true,
            Some(3),
        );

        let expected_added = vec![];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            |_| true,
            Some(2),
        );

        let expected_added = vec![];
        let expected_modified = vec!["dir1"];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            |_| true,
            Some(1),
        );

        let expected_added = vec![];
        let expected_modified = vec![];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            |_| true,
            Some(0),
        );
        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_changed_entry_prune() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash = HgNodeHash::from_str("0c59c8d0da93cbf9d7f4b888f28823ffb2e3e480").unwrap();
        let base_hash = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        // main_hash is a child of base_hash
        // hg st --change .
        // A dir1
        // R dir1/file_1_in_dir1
        // R dir1/file_2_in_dir1
        // R dir1/subdir1/file_1
        // R dir1/subdir1/subsubdir1/file_1
        // R dir1/subdir1/subsubdir2/file_1
        // R dir1/subdir1/subsubdir2/file_2

        let expected_added = vec!["dir1"];
        let expected_deleted = vec!["dir1", "dir1/file_1_in_dir1", "dir1/file_2_in_dir1"];
        do_check_with_pruner(
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
            |entry| {
                let path = entry.get_full_path().clone();
                match path {
                    Some(path) => path.into_iter()
                        .find(|elem| elem.to_bytes() == "subdir1".as_bytes())
                        .is_none(),
                    None => true,
                }
            },
            None,
        );

        let expected_added = vec!["dir1"];
        let expected_deleted = vec![
            "dir1",
            "dir1/file_1_in_dir1",
            "dir1/file_2_in_dir1",
            "dir1/subdir1",
            "dir1/subdir1/file_1",
            "dir1/subdir1/subsubdir1",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2",
            "dir1/subdir1/subsubdir2/file_1",
        ];
        do_check_with_pruner(
            repo,
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
            |entry| {
                let path = entry.get_full_path().clone();
                match path {
                    Some(path) => path.into_iter()
                        .find(|elem| elem.to_bytes() == "file_2".as_bytes())
                        .is_none(),
                    None => true,
                }
            },
            None,
        );

        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_changed_entry_prune_visited() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash_1 = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        let main_hash_2 = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        let base_hash = HgNodeHash::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8").unwrap();

        // VisitedPruner let's us merge stream without producing the same entries twice.
        // o  473b2e
        // |  3
        // |
        // o  ecafdc
        // |  2
        // |
        // o  5a28e2
        //    1
        // $ hg st --change ecafdc
        // A 2
        // A dir1/file_1_in_dir1
        // A dir1/file_2_in_dir1
        // A dir1/subdir1/file_1
        // A dir2/file_1_in_dir2
        // $ hg st --change 473b2e
        // A dir1/subdir1/subsubdir1/file_1
        // A dir1/subdir1/subsubdir2/file_1
        // A dir1/subdir1/subsubdir2/file_2

        let manifest_1 = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash_1));
        let manifest_2 = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash_2));
        let basemanifest = get_root_manifest(repo.clone(), &HgChangesetId::new(base_hash));

        let pruner = visited_pruner();

        let first_stream = changed_entry_stream_with_pruner(
            &manifest_1,
            &basemanifest,
            None,
            pruner.clone(),
            None,
        );
        let second_stream =
            changed_entry_stream_with_pruner(&manifest_2, &basemanifest, None, pruner, None);
        let mut res = spawn(select_all(vec![first_stream, second_stream]).collect());
        let res = res.wait_future().unwrap();
        let unique_len = res.len();
        assert_eq!(unique_len, 15);

        let first_stream = changed_entry_stream(&manifest_1, &basemanifest, None);
        let second_stream = changed_entry_stream(&manifest_2, &basemanifest, None);
        let mut res = spawn(select_all(vec![first_stream, second_stream]).collect());
        let res = res.wait_future().unwrap();
        // Make sure that more entries are produced without VisitedPruner i.e. some entries are
        // returned twice.
        assert!(unique_len < res.len());

        Ok(())
    }).expect("test failed")
}

#[test]
fn test_recursive_changed_entry_prune_visited_no_files() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let repo = Arc::new(many_files_dirs::getrepo(None));
        let main_hash_1 = HgNodeHash::from_str("2f866e7e549760934e31bf0420a873f65100ad63").unwrap();
        let main_hash_2 = HgNodeHash::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4").unwrap();
        let base_hash = HgNodeHash::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8").unwrap();

        // VisitedPruner let's us merge stream without producing the same entries twice.
        // o  473b2e
        // |  3
        // |
        // o  ecafdc
        // |  2
        // |
        // o  5a28e2
        //    1
        // $ hg st --change ecafdc
        // A 2
        // A dir1/file_1_in_dir1
        // A dir1/file_2_in_dir1
        // A dir1/subdir1/file_1
        // A dir2/file_1_in_dir2
        // $ hg st --change 473b2e
        // A dir1/subdir1/subsubdir1/file_1
        // A dir1/subdir1/subsubdir2/file_1
        // A dir1/subdir1/subsubdir2/file_2

        let manifest_1 = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash_1));
        let manifest_2 = get_root_manifest(repo.clone(), &HgChangesetId::new(main_hash_2));
        let basemanifest = get_root_manifest(repo.clone(), &HgChangesetId::new(base_hash));

        let pruner = and_pruner_combinator(&file_pruner, visited_pruner());
        let first_stream = changed_entry_stream_with_pruner(
            &manifest_1,
            &basemanifest,
            None,
            pruner.clone(),
            None,
        );
        let second_stream =
            changed_entry_stream_with_pruner(&manifest_2, &basemanifest, None, pruner, None);
        let mut res = spawn(select_all(vec![first_stream, second_stream]).collect());
        let res = res.wait_future().unwrap();
        let unique_len = res.len();
        assert_eq!(unique_len, 7);

        let first_stream =
            changed_entry_stream_with_pruner(&manifest_1, &basemanifest, None, &file_pruner, None);
        let second_stream =
            changed_entry_stream_with_pruner(&manifest_2, &basemanifest, None, &file_pruner, None);
        let mut res = spawn(select_all(vec![first_stream, second_stream]).collect());
        let res = res.wait_future().unwrap();
        // Make sure that more entries are produced without VisitedPruner i.e. some entries are
        // returned twice.
        assert!(unique_len < res.len());

        Ok(())
    }).expect("test failed")
}

#[test]
fn test_visited_pruner_different_files_same_hash() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let paths = btreemap! {
            "foo1" => (FileType::Regular, "content", HgEntryId::new(NULL_HASH)),
            "foo2" => (FileType::Symlink, "content", HgEntryId::new(NULL_HASH)),
        };
        let root_manifest =
            MockManifest::from_path_hashes(paths, BTreeMap::new()).expect("manifest is valid");

        let pruner = visited_pruner();
        let stream =
            changed_entry_stream_with_pruner(&root_manifest, &EmptyManifest {}, None, pruner, None);
        let mut res = spawn(stream.collect());
        let res = res.wait_future().unwrap();

        assert_eq!(res.len(), 2);
        Ok(())
    }).expect("test failed")
}

#[test]
fn test_file_pruner() {
    async_unit::tokio_unit_test(|| -> Result<_, !> {
        let paths = btreemap! {
            "foo1" => (FileType::Regular, "content", HgEntryId::new(NULL_HASH)),
            "foo2" => (FileType::Symlink, "content", HgEntryId::new(NULL_HASH)),
        };
        let root_manifest =
            MockManifest::from_path_hashes(paths, BTreeMap::new()).expect("manifest is valid");

        let pruner = file_pruner;
        let stream =
            changed_entry_stream_with_pruner(&root_manifest, &EmptyManifest {}, None, pruner, None);
        let mut res = spawn(stream.collect());
        let res = res.wait_future().unwrap();

        assert_eq!(res.len(), 0);
        Ok(())
    }).expect("test failed")
}

#[test]
fn nodehash_option() {
    assert_eq!(NULL_HASH.into_option(), None);
    assert_eq!(HgNodeHash::from(None), NULL_HASH);

    assert_eq!(nodehash::ONES_HASH.into_option(), Some(nodehash::ONES_HASH));
    assert_eq!(
        HgNodeHash::from(Some(nodehash::ONES_HASH)),
        nodehash::ONES_HASH
    );
}
