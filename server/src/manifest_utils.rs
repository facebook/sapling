// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use futures::future::Future;
use futures::stream::{empty, iter_ok, once, Stream};
use futures_ext::{BoxStream, StreamExt};
use std::collections::VecDeque;

use mercurial_types::{Entry, MPath, Manifest};
use mercurial_types::manifest::{Content, Type};

use errors::*;

pub enum EntryStatus {
    Added(Box<Entry + Sync>),
    Deleted(Box<Entry + Sync>),
    // Entries should have the same type. Note - we may change it in future to allow
    // (File, Symlink), (Symlink, Executable) etc
    Modified(Box<Entry + Sync>, Box<Entry + Sync>),
}

pub struct ChangedEntry {
    path: MPath,
    status: EntryStatus,
}

impl ChangedEntry {
    pub fn new_added(path: MPath, entry: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Added(entry),
        }
    }

    pub fn new_deleted(path: MPath, entry: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Deleted(entry),
        }
    }

    pub fn new_modified(path: MPath, left: Box<Entry + Sync>, right: Box<Entry + Sync>) -> Self {
        ChangedEntry {
            path,
            status: EntryStatus::Modified(left, right),
        }
    }
}

/// Given two manifests, returns a difference between them. Difference is a stream of
/// ChangedEntry, each showing whether a file/directory was added, deleted or modified.
/// Note: Modified entry contains only entries of the same type i.e. if a file was replaced
/// with a directory of the same name, then returned stream will contain Deleted file entry,
/// and Added directory entry. The same applies for executable and symlinks, although we may
/// change it in future
#[allow(dead_code)]
pub fn changed_entry_stream(
    to: Box<Manifest>,
    from: Box<Manifest>,
    path: MPath,
) -> BoxStream<ChangedEntry, Error> {
    diff_manifests(path, to, from)
        .map(recursive_changed_entry_stream)
        .flatten()
        .boxify()
}

/// Given a ChangedEntry, return a stream that consists of this entry, and all subentries
/// that differ. If input isn't a tree, then a stream with a single entry is returned, otherwise
/// subtrees are recursively compared.
fn recursive_changed_entry_stream(changed_entry: ChangedEntry) -> BoxStream<ChangedEntry, Error> {
    match changed_entry.status {
        EntryStatus::Added(entry) => recursive_entry_stream(changed_entry.path, entry)
            .map(|(path, entry)| ChangedEntry::new_added(path, entry))
            .boxify(),
        EntryStatus::Deleted(entry) => recursive_entry_stream(changed_entry.path, entry)
            .map(|(path, entry)| ChangedEntry::new_deleted(path, entry))
            .boxify(),
        EntryStatus::Modified(left, right) => {
            debug_assert!(left.get_type() == right.get_type());

            let substream = if left.get_type() == Type::Tree {
                let contents = left.get_content().join(right.get_content());
                let path = changed_entry.path.clone();
                let entry_path = left.get_mpath().clone();

                let substream = contents
                    .map(move |(left_content, right_content)| {
                        let left_manifest = get_tree_content(left_content);
                        let right_manifest = get_tree_content(right_content);

                        diff_manifests(path.join(&entry_path), left_manifest, right_manifest)
                            .map(recursive_changed_entry_stream)
                    })
                    .flatten_stream()
                    .flatten();

                substream.boxify()
            } else {
                empty().boxify()
            };

            let current_entry = once(Ok(ChangedEntry::new_modified(
                changed_entry.path.clone(),
                left,
                right,
            )));
            current_entry.chain(substream).boxify()
        }
    }
}

/// Given an entry and path from the root of the repo to this entry, returns all subentries with
/// their path from the root of the repo.
/// For a non-tree entry returns a stream with a single (entry, path) pair.
fn recursive_entry_stream(
    rootpath: MPath,
    entry: Box<Entry + Sync>,
) -> BoxStream<(MPath, Box<Entry + Sync>), Error> {
    let subentries = match entry.get_type() {
        Type::File | Type::Symlink | Type::Executable => empty().boxify(),
        Type::Tree => {
            let entry_basename = entry.get_mpath().clone();
            let path = rootpath.join(&entry_basename);

            entry
                .get_content()
                .map(|content| {
                    get_tree_content(content)
                        .list()
                        .map(move |entry| recursive_entry_stream(path.clone(), entry))
                })
                .flatten_stream()
                .flatten()
                .boxify()
        }
    };

    once(Ok((rootpath, entry))).chain(subentries).boxify()
}

/// Difference between manifests, non-recursive.
/// It fetches manifest content, sorts it and compares.
fn diff_manifests(
    path: MPath,
    left: Box<Manifest>,
    right: Box<Manifest>,
) -> BoxStream<ChangedEntry, Error> {
    let left_vec_future = left.list().collect();
    let right_vec_future = right.list().collect();

    left_vec_future
        .join(right_vec_future)
        .map(|(left, right)| iter_ok(diff_sorted_vecs(path, left, right).into_iter()))
        .flatten_stream()
        .boxify()
}

/// Compares vectors of entries and returns the difference
fn diff_sorted_vecs(
    path: MPath,
    left: Vec<Box<Entry + Sync>>,
    right: Vec<Box<Entry + Sync>>,
) -> Vec<ChangedEntry> {
    let mut left = VecDeque::from(left);
    let mut right = VecDeque::from(right);

    let mut res = vec![];
    loop {
        match (left.pop_front(), right.pop_front()) {
            (Some(left_entry), Some(right_entry)) => {
                let left_path = left_entry.get_mpath().to_vec();
                let right_path = right_entry.get_mpath().to_vec();

                if left_path < right_path {
                    res.push(ChangedEntry::new_added(path.clone(), left_entry));
                    right.push_front(right_entry);
                } else if left_path > right_path {
                    res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
                    left.push_front(left_entry);
                } else {
                    if left_entry.get_type() == right_entry.get_type() {
                        if left_entry.get_hash() != right_entry.get_hash() {
                            res.push(ChangedEntry::new_modified(
                                path.clone(),
                                left_entry,
                                right_entry,
                            ));
                        }
                    } else {
                        res.push(ChangedEntry::new_added(path.clone(), left_entry));
                        res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
                    }
                }
            }

            (Some(left_entry), None) => {
                res.push(ChangedEntry::new_added(path.clone(), left_entry));
            }

            (None, Some(right_entry)) => {
                res.push(ChangedEntry::new_deleted(path.clone(), right_entry));
            }
            (None, None) => {
                break;
            }
        }
    }

    res
}

fn get_tree_content(content: Content) -> Box<Manifest> {
    match content {
        Content::Tree(manifest) => manifest,
        _ => panic!("Tree entry was expected"),
    }
}

#[cfg(test)]
mod test {

    use super::*;
    use blobrepo::BlobRepo;
    use futures::executor::spawn;
    use many_files_dirs;
    use mercurial_types::{MPath, RepoPath};
    use mercurial_types::nodehash::{EntryId, NodeHash};
    use mercurial_types_mocks::manifest::{ContentFactory, MockEntry};
    use std::convert::TryFrom;
    use std::iter::repeat;
    use std::str::FromStr;
    use std::sync::Arc;

    fn get_root_manifest(repo: Arc<BlobRepo>, hash: &NodeHash) -> Box<Manifest> {
        let cs = repo.get_changeset_by_nodeid(&hash).wait().unwrap();
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
        let mut stream = spawn(changed_entry_stream(manifest, basemanifest, MPath::empty()));
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
                    paths_added.push(changed_entry.path.join(entry.get_mpath()));
                }
                EntryStatus::Deleted(entry) => {
                    paths_deleted.push(changed_entry.path.join(entry.get_mpath()));
                }
                EntryStatus::Modified(left_entry, right_entry) => {
                    assert_eq!(left_entry.get_type(), right_entry.get_type());
                    assert_eq!(
                        left_entry.get_mpath().to_vec(),
                        right_entry.get_mpath().to_vec()
                    );
                    paths_modified.push(changed_entry.path.join(left_entry.get_mpath()));
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
            let manifest = get_root_manifest(repo.clone(), &main_hash);
            let base_manifest = get_root_manifest(repo.clone(), &base_hash);

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
            let manifest = get_root_manifest(repo.clone(), &base_hash);
            let base_manifest = get_root_manifest(repo.clone(), &main_hash);

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
        let repo = Arc::new(many_files_dirs::getrepo());
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
        let repo = Arc::new(many_files_dirs::getrepo());
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
        let repo = Arc::new(many_files_dirs::getrepo());
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
}
