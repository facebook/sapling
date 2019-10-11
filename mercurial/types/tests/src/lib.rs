/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]
#![feature(never_type)]

use blobrepo::BlobRepo;
use bytes::Bytes;
use context::CoreContext;
use failure::{err_msg, Error};
use fbinit::FacebookInit;
use fixtures::{linear, many_files_dirs};
use futures::executor::spawn;
use futures::{Future, Stream};
use maplit::btreemap;
use mercurial_types::{
    blobs::{filenode_lookup::FileNodeIdPointer, File, LFSContent, META_MARKER, META_SZ},
    manifest::{Content, HgEmptyManifest},
    manifest_utils::{
        changed_entry_stream_with_pruner, diff_sorted_vecs, ChangedEntry, DeletedPruner,
        EntryStatus, FilePruner, NoopPruner, Pruner,
    },
    nodehash::{HgChangesetId, HgNodeHash},
    Changeset, FileBytes, FileType, HgEntry, HgFileNodeId, HgManifest, MPath, RepoPath, Type,
    NULL_HASH,
};
use mercurial_types_mocks::{
    manifest::{ContentFactory, MockEntry, MockManifest},
    nodehash::{self, FOURS_FNID, ONES_FNID, THREES_FNID, TWOS_FNID},
};
use mononoke_types::hash::Sha256;
use mononoke_types_mocks::contentid::{ONES_CTID, TWOS_CTID};
use quickcheck::quickcheck;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    iter::repeat,
    str::FromStr,
    sync::Arc,
};

fn get_root_manifest(
    ctx: CoreContext,
    repo: Arc<BlobRepo>,
    changesetid: HgChangesetId,
) -> Box<dyn HgManifest> {
    let cs = repo
        .get_changeset_by_changesetid(ctx.clone(), changesetid)
        .wait()
        .unwrap();
    let manifestid = cs.manifestid();
    repo.get_manifest_by_nodeid(ctx, manifestid).wait().unwrap()
}

fn get_hash(c: char) -> HgNodeHash {
    let hash: String = repeat(c).take(40).collect();
    HgNodeHash::from_str(&hash).unwrap()
}

fn get_entry(ty: Type, hash: HgNodeHash, path: RepoPath) -> Box<dyn HgEntry + Sync> {
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
    ctx: CoreContext,
    manifest: Box<dyn HgManifest>,
    basemanifest: Box<dyn HgManifest>,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: Option<usize>,
) -> Vec<ChangedEntry> {
    let mut stream = spawn(changed_entry_stream_with_pruner(
        ctx,
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
            EntryStatus::Added(_) => {
                paths_added.push(changed_entry.get_full_path());
            }
            EntryStatus::Deleted(_) => {
                paths_deleted.push(changed_entry.get_full_path());
            }
            EntryStatus::Modified {
                ref to_entry,
                ref from_entry,
            } => {
                assert_eq!(to_entry.get_type(), from_entry.get_type());
                paths_modified.push(changed_entry.get_full_path());
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

fn do_check_with_pruner(
    ctx: CoreContext,
    repo: Arc<BlobRepo>,
    main_hash: HgNodeHash,
    base_hash: HgNodeHash,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
    pruner: impl Pruner + Send + Clone + 'static,
    max_depth: Option<usize>,
) {
    {
        let manifest = get_root_manifest(ctx.clone(), repo.clone(), HgChangesetId::new(main_hash));
        let base_manifest =
            get_root_manifest(ctx.clone(), repo.clone(), HgChangesetId::new(base_hash));

        let res = find_changed_entry_status_stream(
            ctx.clone(),
            manifest,
            base_manifest,
            pruner.clone(),
            max_depth,
        );

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
        let manifest = get_root_manifest(ctx.clone(), repo.clone(), HgChangesetId::new(base_hash));
        let base_manifest =
            get_root_manifest(ctx.clone(), repo.clone(), HgChangesetId::new(main_hash));

        let res = find_changed_entry_status_stream(
            ctx.clone(),
            manifest,
            base_manifest,
            pruner,
            max_depth,
        );

        check_changed_paths(
            res,
            expected_deleted.clone(),
            expected_added.clone(),
            expected_modified.clone(),
        );
    }
}

fn do_check(
    ctx: CoreContext,
    repo: Arc<BlobRepo>,
    main_hash: HgNodeHash,
    base_hash: HgNodeHash,
    expected_added: Vec<&str>,
    expected_deleted: Vec<&str>,
    expected_modified: Vec<&str>,
) {
    do_check_with_pruner(
        ctx,
        repo,
        main_hash,
        base_hash,
        expected_added,
        expected_deleted,
        expected_modified,
        NoopPruner,
        None,
    )
}

#[fbinit::test]
fn test_recursive_changed_entry_stream_linear(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(linear::getrepo(fb));
        let main_hash = HgNodeHash::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb").unwrap();
        let base_hash = HgNodeHash::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap();

        let expected_modified = vec!["10"];
        do_check(
            ctx.clone(),
            repo,
            main_hash,
            base_hash,
            vec![],
            vec![],
            expected_modified,
        );
        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_recursive_changed_entry_stream_simple(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(many_files_dirs::getrepo(fb));
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
        do_check(
            ctx.clone(),
            repo,
            main_hash,
            base_hash,
            expected_added,
            vec![],
            vec![],
        );
        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_recursive_changed_entry_stream_changed_dirs(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(many_files_dirs::getrepo(fb));
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
            ctx.clone(),
            repo,
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
        );
        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_recursive_changed_entry_stream_dirs_replaced_with_file(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(many_files_dirs::getrepo(fb));
        let main_hash = HgNodeHash::from_str("051946ed218061e925fb120dac02634f9ad40ae2").unwrap();
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
            ctx.clone(),
            repo,
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
        );
        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_depth_parameter(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(many_files_dirs::getrepo(fb));
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
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            NoopPruner,
            Some(4),
        );

        let expected_added = vec!["dir1/subdir1/subsubdir1", "dir1/subdir1/subsubdir2"];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check_with_pruner(
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            NoopPruner,
            Some(3),
        );

        let expected_added = vec![];
        let expected_modified = vec!["dir1", "dir1/subdir1"];
        do_check_with_pruner(
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            NoopPruner,
            Some(2),
        );

        let expected_added = vec![];
        let expected_modified = vec!["dir1"];
        do_check_with_pruner(
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            NoopPruner,
            Some(1),
        );

        let expected_added = vec![];
        let expected_modified = vec![];
        do_check_with_pruner(
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            vec![],
            expected_modified,
            NoopPruner,
            Some(0),
        );
        Ok(())
    })
    .expect("test failed")
}

#[derive(Clone)]
struct TestFuncPruner<F> {
    func: F,
}

impl<F> Pruner for TestFuncPruner<F>
where
    F: FnMut(&ChangedEntry) -> bool,
{
    fn keep(&mut self, entry: &ChangedEntry) -> bool {
        (self.func)(entry)
    }
}

#[fbinit::test]
fn test_recursive_changed_entry_prune(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Arc::new(many_files_dirs::getrepo(fb));
        let main_hash = HgNodeHash::from_str("051946ed218061e925fb120dac02634f9ad40ae2").unwrap();
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
            ctx.clone(),
            repo.clone(),
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
            TestFuncPruner {
                func: |entry: &ChangedEntry| {
                    let path = entry.get_full_path().clone();
                    match path {
                        Some(path) => path
                            .into_iter()
                            .find(|elem| elem.to_bytes() == "subdir1".as_bytes())
                            .is_none(),
                        None => true,
                    }
                },
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
            ctx.clone(),
            repo,
            main_hash,
            base_hash,
            expected_added,
            expected_deleted,
            vec![],
            TestFuncPruner {
                func: |entry: &ChangedEntry| {
                    let path = entry.get_full_path().clone();
                    match path {
                        Some(path) => path
                            .into_iter()
                            .find(|elem| elem.to_bytes() == "file_2".as_bytes())
                            .is_none(),
                        None => true,
                    }
                },
            },
            None,
        );

        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_file_pruner(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let paths = btreemap! {
            "foo1" => (FileType::Regular, "content", NULL_HASH),
            "foo2" => (FileType::Symlink, "content", NULL_HASH),
        };
        let root_manifest =
            MockManifest::from_path_hashes(paths, BTreeMap::new()).expect("manifest is valid");

        let pruner = FilePruner;
        let stream = changed_entry_stream_with_pruner(
            ctx.clone(),
            &root_manifest,
            &HgEmptyManifest {},
            None,
            pruner,
            None,
        );
        let mut res = spawn(stream.collect());
        let res = res.wait_future().unwrap();

        assert_eq!(res.len(), 0);
        Ok(())
    })
    .expect("test failed")
}

#[fbinit::test]
fn test_deleted_pruner(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || -> Result<_, !> {
        let ctx = CoreContext::test_mock(fb);
        let paths = btreemap! {
            "foo1" => (FileType::Regular, "content", NULL_HASH),
            "foo2" => (FileType::Symlink, "content", NULL_HASH),
        };
        let root_manifest =
            MockManifest::from_path_hashes(paths, BTreeMap::new()).expect("manifest is valid");

        let pruner = DeletedPruner;
        let stream = changed_entry_stream_with_pruner(
            ctx.clone(),
            &root_manifest,
            &HgEmptyManifest {},
            None,
            pruner,
            None,
        );
        let mut res = spawn(stream.collect());
        let res = res.wait_future().unwrap();

        assert_eq!(
            res.len(),
            2,
            "deleted pruner shouldn't remove added entries"
        );

        let pruner = DeletedPruner;
        let stream = changed_entry_stream_with_pruner(
            ctx.clone(),
            &HgEmptyManifest {},
            &root_manifest,
            None,
            pruner,
            None,
        );
        let mut res = spawn(stream.collect());
        let res = res.wait_future().unwrap();

        assert_eq!(res.len(), 0, "deleted pruner should remove deleted entries");
        Ok(())
    })
    .expect("test failed")
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

#[test]
fn nodehash_display_opt() {
    assert_eq!(
        format!("{}", HgNodeHash::display_opt(Some(&nodehash::ONES_HASH))),
        "1111111111111111111111111111111111111111"
    );
    assert_eq!(format!("{}", HgNodeHash::display_opt(None)), "(none)");
}

#[test]
fn changeset_id_display_opt() {
    assert_eq!(
        format!("{}", HgChangesetId::display_opt(Some(&nodehash::ONES_CSID))),
        "1111111111111111111111111111111111111111"
    );
    assert_eq!(format!("{}", HgChangesetId::display_opt(None)), "(none)");
}

#[test]
fn extract_meta_sz() {
    assert_eq!(META_SZ, META_MARKER.len())
}

#[test]
fn extract_meta_0() {
    const DATA: &[u8] = b"foo - no meta";

    assert_eq!(File::extract_meta(DATA), (&[][..], 0));
}

#[test]
fn extract_meta_1() {
    const DATA: &[u8] = b"\x01\n\x01\nfoo - empty meta";

    assert_eq!(File::extract_meta(DATA), (&[][..], 4));
}

#[test]
fn extract_meta_2() {
    const DATA: &[u8] = b"\x01\nabc\x01\nfoo - some meta";

    assert_eq!(File::extract_meta(DATA), (&b"abc"[..], 7));
}

#[test]
fn extract_meta_3() {
    const DATA: &[u8] = b"\x01\nfoo - bad unterminated meta";

    assert_eq!(File::extract_meta(DATA), (&[][..], 2));
}

#[test]
fn extract_meta_4() {
    const DATA: &[u8] = b"\x01\n\x01\n\x01\nfoo - bad unterminated meta";

    assert_eq!(File::extract_meta(DATA), (&[][..], 4));
}

#[test]
fn extract_meta_5() {
    const DATA: &[u8] = b"\x01\n\x01\n";

    assert_eq!(File::extract_meta(DATA), (&[][..], 4));
}

#[test]
fn parse_meta_0() {
    const DATA: &[u8] = b"foo - no meta";

    assert!(File::parse_meta(DATA).is_empty())
}

#[test]
fn test_meta_1() {
    const DATA: &[u8] = b"\x01\n\x01\nfoo - empty meta";

    assert!(File::parse_meta(DATA).is_empty())
}

#[test]
fn test_meta_2() {
    const DATA: &[u8] = b"\x01\nfoo: bar\x01\nfoo - empty meta";

    let kv: Vec<_> = File::parse_meta(DATA).into_iter().collect();

    assert_eq!(kv, vec![(b"foo".as_ref(), b"bar".as_ref())])
}

#[test]
fn test_meta_3() {
    const DATA: &[u8] = b"\x01\nfoo: bar\nblim: blop: blap\x01\nfoo - empty meta";

    let mut kv: Vec<_> = File::parse_meta(DATA).into_iter().collect();
    kv.as_mut_slice().sort();

    assert_eq!(
        kv,
        vec![
            (b"blim".as_ref(), b"blop: blap".as_ref()),
            (b"foo".as_ref(), b"bar".as_ref()),
        ]
    )
}

#[test]
fn test_hash_meta_delimiter_only_0() {
    const DELIMITER: &[u8] = b"DELIMITER";
    const DATA: &[u8] = b"DELIMITER\n";

    let mut kv: Vec<_> = File::parse_to_hash_map(DATA, DELIMITER)
        .into_iter()
        .collect();
    kv.as_mut_slice().sort();
    assert_eq!(kv, vec![(b"".as_ref(), b"".as_ref())])
}

#[test]
fn test_hash_meta_delimiter_only_1() {
    const DELIMITER: &[u8] = b"DELIMITER";
    const DATA: &[u8] = b"DELIMITER";

    let mut kv: Vec<_> = File::parse_to_hash_map(DATA, DELIMITER)
        .into_iter()
        .collect();
    kv.as_mut_slice().sort();
    assert_eq!(kv, vec![(b"".as_ref(), b"".as_ref())])
}

#[test]
fn test_hash_meta_delimiter_short_0() {
    const DELIMITER: &[u8] = b"DELIMITER";
    const DATA: &[u8] = b"DELIM";

    let mut kv: Vec<_> = File::parse_to_hash_map(DATA, DELIMITER)
        .into_iter()
        .collect();
    assert!(kv.as_mut_slice().is_empty())
}

#[test]
fn test_hash_meta_delimiter_short_1() {
    const DELIMITER: &[u8] = b"DELIMITER";
    const DATA: &[u8] = b"\n";

    let mut kv: Vec<_> = File::parse_to_hash_map(DATA, DELIMITER)
        .into_iter()
        .collect();
    assert!(kv.as_mut_slice().is_empty())
}

#[test]
fn test_parse_to_hash_map_long_delimiter() {
    const DATA: &[u8] = b"x\nfooDELIMITERbar\nfoo1DELIMITERbar1";
    const DELIMITER: &[u8] = b"DELIMITER";
    let mut kv: Vec<_> = File::parse_to_hash_map(DATA, DELIMITER)
        .into_iter()
        .collect();
    kv.as_mut_slice().sort();
    assert_eq!(
        kv,
        vec![
            (b"foo".as_ref(), b"bar".as_ref()),
            (b"foo1".as_ref(), b"bar1".as_ref()),
        ]
    )
}

#[test]
fn generate_metadata_0() {
    const FILE_BYTES: &[u8] = b"foobar";
    let file_bytes = FileBytes(Bytes::from(FILE_BYTES));
    let mut out: Vec<u8> = vec![];
    File::generate_metadata(None, &file_bytes, &mut out).expect("Vec::write_all should succeed");
    assert_eq!(out.as_slice(), &b""[..]);

    let mut out: Vec<u8> = vec![];
    File::generate_metadata(
        Some(&(MPath::new("foo").unwrap(), nodehash::ONES_FNID)),
        &file_bytes,
        &mut out,
    )
    .expect("Vec::write_all should succeed");
    assert_eq!(
        out.as_slice(),
        &b"\x01\ncopy: foo\ncopyrev: 1111111111111111111111111111111111111111\n\x01\n"[..]
    );
}

#[test]
fn generate_metadata_1() {
    // The meta marker in the beginning should cause metadata to unconditionally be emitted.
    const FILE_BYTES: &[u8] = b"\x01\nfoobar";
    let file_bytes = FileBytes(Bytes::from(FILE_BYTES));
    let mut out: Vec<u8> = vec![];
    File::generate_metadata(None, &file_bytes, &mut out).expect("Vec::write_all should succeed");
    assert_eq!(out.as_slice(), &b"\x01\n\x01\n"[..]);

    let mut out: Vec<u8> = vec![];
    File::generate_metadata(
        Some(&(MPath::new("foo").unwrap(), nodehash::ONES_FNID)),
        &file_bytes,
        &mut out,
    )
    .expect("Vec::write_all should succeed");
    assert_eq!(
        out.as_slice(),
        &b"\x01\ncopy: foo\ncopyrev: 1111111111111111111111111111111111111111\n\x01\n"[..]
    );
}

#[test]
fn test_get_lfs_hash_map() {
    const DATA: &[u8] = b"version https://git-lfs.github.com/spec/v1\noid sha256:27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97\nsize 17";

    let mut kv: Vec<_> = File::parse_content_to_lfs_hash_map(DATA)
        .into_iter()
        .collect();
    kv.as_mut_slice().sort();

    assert_eq!(
        kv,
        vec![
            (
                b"oid".as_ref(),
                b"sha256:27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97".as_ref(),
            ),
            (b"size".as_ref(), b"17".as_ref()),
            (
                b"version".as_ref(),
                b"https://git-lfs.github.com/spec/v1".as_ref(),
            ),
        ]
    )
}

#[test]
fn test_get_lfs_struct_0() {
    let mut kv = HashMap::new();
    kv.insert(
        b"version".as_ref(),
        b"https://git-lfs.github.com/spec/v1".as_ref(),
    );
    kv.insert(
        b"oid".as_ref(),
        b"sha256:27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97".as_ref(),
    );
    kv.insert(b"size".as_ref(), b"17".as_ref());
    let lfs = File::get_lfs_struct(&kv);

    assert_eq!(
        lfs.unwrap(),
        LFSContent::new(
            "https://git-lfs.github.com/spec/v1".to_string(),
            Sha256::from_str("27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97")
                .unwrap(),
            17,
            None,
        )
    )
}

#[test]
fn test_get_lfs_struct_wrong_small_sha256() {
    let mut kv = HashMap::new();
    kv.insert(
        b"version".as_ref(),
        b"https://git-lfs.github.com/spec/v1".as_ref(),
    );
    kv.insert(b"oid".as_ref(), b"sha256:123".as_ref());
    kv.insert(b"size".as_ref(), b"17".as_ref());
    let lfs = File::get_lfs_struct(&kv);

    assert_eq!(lfs.is_err(), true)
}

#[test]
fn test_get_lfs_struct_wrong_size() {
    let mut kv = HashMap::new();
    kv.insert(
        b"version".as_ref(),
        b"https://git-lfs.github.com/spec/v1".as_ref(),
    );
    kv.insert(
        b"oid".as_ref(),
        b"sha256:27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97".as_ref(),
    );
    kv.insert(b"size".as_ref(), b"wrong_size_length".as_ref());
    let lfs = File::get_lfs_struct(&kv);

    assert_eq!(lfs.is_err(), true)
}

#[test]
fn test_get_lfs_struct_non_all_mandatory_fields() {
    let mut kv = HashMap::new();
    kv.insert(
        b"oid".as_ref(),
        b"sha256:27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97".as_ref(),
    );
    let lfs = File::get_lfs_struct(&kv);

    assert_eq!(lfs.is_err(), true)
}

#[test]
fn test_roundtrip_lfs_content() {
    let oid = Sha256::from_str("27c0a92fc51290e3227bea4dd9e780c5035f017de8d5ddfa35b269ed82226d97")
        .unwrap();
    let size = 10;

    let generated_file = File::generate_lfs_file(oid, size, None).unwrap();
    let lfs_struct = File::data_only(generated_file).get_lfs_content().unwrap();

    let expected_lfs_struct = LFSContent::new(
        "https://git-lfs.github.com/spec/v1".to_string(),
        oid,
        size,
        None,
    );
    assert_eq!(lfs_struct, expected_lfs_struct)
}

quickcheck! {
    fn copy_info_roundtrip(
        copy_info: Option<(MPath, HgFileNodeId)>,
        file_bytes: FileBytes
    ) -> bool {
        let mut buf = Vec::new();
        let result = File::generate_metadata(copy_info.as_ref(), &file_bytes, &mut buf)
            .and_then(|_| {
                File::extract_copied_from(&buf)
            });
        match result {
            Ok(out_copy_info) => copy_info == out_copy_info,
            _ => {
                false
            }
        }
    }

    fn lfs_copy_info_roundtrip(
        oid: Sha256,
        size: u64,
        copy_from: Option<(MPath, HgFileNodeId)>
    ) -> bool {
        let result = File::generate_lfs_file(oid, size, copy_from.clone())
            .and_then(|bytes| File::data_only(bytes).get_lfs_content());

        match result {
            Ok(result) => result.oid() == oid && result.size() == size && result.copy_from() == copy_from,
            _ => false,
        }
    }
}

#[test]
fn test_hashes_are_unique() -> Result<(), Error> {
    let mut h = HashSet::new();

    for content_id in [ONES_CTID, TWOS_CTID].iter() {
        for p1 in [Some(ONES_FNID), Some(TWOS_FNID), None].iter() {
            for p2 in [Some(THREES_FNID), Some(FOURS_FNID), None].iter() {
                let path1 = RepoPath::file("path")?
                    .into_mpath()
                    .ok_or(err_msg("path1"))?;

                let path2 = RepoPath::file("path/2")?
                    .into_mpath()
                    .ok_or(err_msg("path2"))?;

                let path3 = RepoPath::file("path2")?
                    .into_mpath()
                    .ok_or(err_msg("path3"))?;

                for copy_path in [path1, path2, path3].iter() {
                    for copy_parent in [ONES_FNID, TWOS_FNID, THREES_FNID].iter() {
                        let copy_info = Some((copy_path.clone(), copy_parent.clone()));

                        let ptr = FileNodeIdPointer::new(&content_id, &copy_info, p1, p2);
                        assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                        h.insert(ptr);

                        if p1 == p2 {
                            continue;
                        }

                        let ptr = FileNodeIdPointer::new(&content_id, &copy_info, p2, p1);
                        assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                        h.insert(ptr);
                    }
                }

                let ptr = FileNodeIdPointer::new(&content_id, &None, p1, p2);
                assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                h.insert(ptr);

                if p1 == p2 {
                    continue;
                }

                let ptr = FileNodeIdPointer::new(&content_id, &None, p2, p1);
                assert!(!h.contains(&ptr), format!("Duplicate entry: {:?}", ptr));
                h.insert(ptr);
            }
        }
    }

    Ok(())
}
