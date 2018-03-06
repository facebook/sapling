// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(try_from)]

extern crate blobrepo;
extern crate futures;
extern crate many_files_dirs;
extern crate mercurial_types;
extern crate mercurial_types_mocks;

use blobrepo::BlobRepo;
use futures::Future;
use futures::executor::spawn;
use mercurial_types::{Changeset, Entry, MPath, Manifest, RepoPath, Type, NULL_HASH};
use mercurial_types::manifest::Content;
use mercurial_types::manifest_utils::{changed_entry_stream, diff_sorted_vecs, ChangedEntry,
                                      EntryStatus};
use mercurial_types::nodehash::{ChangesetId, EntryId, NodeHash};
use mercurial_types_mocks::manifest::{ContentFactory, MockEntry};
use mercurial_types_mocks::nodehash;
use std::convert::TryFrom;
use std::iter::repeat;
use std::str::FromStr;
use std::sync::Arc;

fn get_root_manifest(repo: Arc<BlobRepo>, changesetid: &ChangesetId) -> Box<Manifest> {
    let cs = repo.get_changeset_by_changesetid(changesetid)
        .wait()
        .unwrap();
    let manifestid = cs.manifestid();
    repo.get_manifest_by_nodeid(&manifestid.into_nodehash())
        .wait()
        .unwrap()
}

fn get_hash(c: char) -> EntryId {
    let hash: String = repeat(c).take(40).collect();
    EntryId::new(NodeHash::from_str(&hash).unwrap())
}

fn get_entry(ty: Type, hash: EntryId, path: RepoPath) -> Box<Entry + Sync> {
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
            EntryStatus::Modified(..) => modified += 1,
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

    let left_entry = get_entry(Type::File, get_hash('1'), path.clone());
    let right_entry = get_entry(Type::File, get_hash('2'), path.clone());
    let res = diff_sorted_vecs(MPath::empty(), vec![left_entry], vec![right_entry]);

    assert_eq!(res.len(), 1);
    let (_, modified, _) = count_entries(&res);
    assert_eq!(modified, 1);

    // With different types we should get added and deleted entries
    let left_entry = get_entry(Type::File, get_hash('1'), path.clone());
    let right_entry = get_entry(Type::Tree, get_hash('2'), path.clone());
    let res = diff_sorted_vecs(MPath::empty(), vec![left_entry], vec![right_entry]);

    assert_eq!(res.len(), 2);
    let (added, _, deleted) = count_entries(&res);
    assert_eq!(added, 1);
    assert_eq!(deleted, 1);
}

#[test]
fn test_diff_sorted_vecs_added_deleted() {
    let left_path = RepoPath::file("file1.txt").unwrap();
    let right_path = RepoPath::file("file2.txt").unwrap();

    let left_entry = get_entry(Type::File, get_hash('1'), left_path);
    let right_entry = get_entry(Type::File, get_hash('2'), right_path);
    let res = diff_sorted_vecs(MPath::empty(), vec![left_entry], vec![right_entry]);

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

        let left_entry_first = get_entry(Type::File, get_hash('1'), left_path_first);
        let left_entry_second = get_entry(Type::File, get_hash('2'), path_second.clone());
        let right_entry = get_entry(Type::File, get_hash('2'), path_second);

        let res = diff_sorted_vecs(
            MPath::empty(),
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

        let left_entry_first = get_entry(Type::File, get_hash('1'), path_first.clone());
        let left_entry_second = get_entry(Type::File, get_hash('2'), left_path_second);
        let right_entry = get_entry(Type::File, get_hash('1'), path_first);

        let res = diff_sorted_vecs(
            MPath::empty(),
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

    let entry = get_entry(Type::File, get_hash('1'), path);
    let res = diff_sorted_vecs(MPath::empty(), vec![entry], vec![]);

    assert_eq!(res.len(), 1);
    let (added, ..) = count_entries(&res);
    assert_eq!(added, 1);
}

fn find_changed_entry_status_stream(
    manifest: Box<Manifest>,
    basemanifest: Box<Manifest>,
) -> Vec<ChangedEntry> {
    let mut stream = spawn(changed_entry_stream(
        &manifest,
        &basemanifest,
        MPath::empty(),
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
                paths_added.push(changed_entry.path.join_element(&entry.get_name()));
            }
            EntryStatus::Deleted(entry) => {
                paths_deleted.push(changed_entry.path.join_element(&entry.get_name()));
            }
            EntryStatus::Modified(left_entry, right_entry) => {
                assert_eq!(left_entry.get_type(), right_entry.get_type());
                paths_modified.push(changed_entry.path.join_element(&left_entry.get_name()));
            }
        }
    }

    fn compare(change_name: &str, mut actual: Vec<MPath>, expected: Vec<&str>) {
        actual.sort_by(|a, b| a.to_vec().cmp(&b.to_vec()));
        let mut expected: Vec<_> = expected
            .into_iter()
            .map(|s| MPath::try_from(s).unwrap())
            .collect();
        expected.sort_by(|a, b| a.to_vec().cmp(&b.to_vec()));

        let actual_strs: Vec<_> = actual
            .iter()
            .map(|path| String::from_utf8(path.to_vec()).unwrap())
            .collect();
        let expected_strs: Vec<_> = expected
            .iter()
            .map(|path| String::from_utf8(path.to_vec()).unwrap())
            .collect();

        assert_eq!(
            actual, expected,
            "{} check failed! expected: {:?}, got: {:?}",
            change_name, expected_strs, actual_strs
        );
    }

    compare("added", paths_added, expected_added);
    compare("deleted", paths_deleted, expected_deleted);
    compare("modified", paths_modified, expected_modified);
}

fn do_check(
    repo: Arc<BlobRepo>,
    main_hash: NodeHash,
    base_hash: NodeHash,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
) {
    {
        let manifest = get_root_manifest(repo.clone(), &ChangesetId::new(main_hash));
        let base_manifest = get_root_manifest(repo.clone(), &ChangesetId::new(base_hash));

        let res = find_changed_entry_status_stream(manifest, base_manifest);

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
        let manifest = get_root_manifest(repo.clone(), &ChangesetId::new(base_hash));
        let base_manifest = get_root_manifest(repo.clone(), &ChangesetId::new(main_hash));

        let res = find_changed_entry_status_stream(manifest, base_manifest);

        check_changed_paths(
            res,
            expected_deleted.clone(),
            expected_added.clone(),
            expected_modified.clone(),
        );
    }
}

#[test]
fn test_recursive_changed_entry_stream_simple() {
    let repo = Arc::new(many_files_dirs::getrepo(None));
    let main_hash = NodeHash::from_str("ecafdc4a4b6748b7a7215c6995f14c837dc1ebec").unwrap();
    let base_hash = NodeHash::from_str("5a28e25f924a5d209b82ce0713d8d83e68982bc8").unwrap();
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
}

#[test]
fn test_recursive_changed_entry_stream_changed_dirs() {
    let repo = Arc::new(many_files_dirs::getrepo(None));
    let main_hash = NodeHash::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
    let base_hash = NodeHash::from_str("ecafdc4a4b6748b7a7215c6995f14c837dc1ebec").unwrap();
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
}

#[test]
fn test_recursive_changed_entry_stream_dirs_replaced_with_file() {
    let repo = Arc::new(many_files_dirs::getrepo(None));
    let main_hash = NodeHash::from_str("a6cb7dddec32acaf9a28db46cdb3061682155531").unwrap();
    let base_hash = NodeHash::from_str("473b2e715e0df6b2316010908879a3c78e275dd9").unwrap();
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
}

#[test]
fn nodehash_option() {
    assert_eq!(NULL_HASH.into_option(), None);
    assert_eq!(NodeHash::from(None), NULL_HASH);

    assert_eq!(nodehash::ONES_HASH.into_option(), Some(nodehash::ONES_HASH));
    assert_eq!(
        NodeHash::from(Some(nodehash::ONES_HASH)),
        nodehash::ONES_HASH
    );
}
