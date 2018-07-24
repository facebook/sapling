// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate async_unit;
extern crate bytes;
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate quickcheck;
extern crate scuba_ext;

extern crate blobrepo;
extern crate blobstore;
extern crate changesets;
extern crate dbbookmarks;
extern crate many_files_dirs;
extern crate mercurial;
extern crate mercurial_types;
extern crate mercurial_types_mocks;
extern crate merge_uneven;
extern crate mononoke_types;

use failure::Error;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use quickcheck::{quickcheck, Arbitrary, Gen, TestResult, Testable};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::marker::PhantomData;

use blobrepo::{compute_changed_files, BlobRepo, ErrorKind};
use mercurial_types::{manifest, Changeset, Entry, FileType, HgChangesetId, HgEntryId,
                      HgManifestId, HgNodeHash, HgParents, MPath, MPathElement, RepoPath};
use mononoke_types::{BonsaiChangeset, ChangesetId, ContentId, DateTime, FileChange, FileContents,
                     MononokeId};
use mononoke_types::bonsai_changeset::BonsaiChangesetMut;

#[macro_use]
mod utils;
mod memory_manifest;

use utils::{create_changeset_no_parents, create_changeset_one_parent, get_empty_eager_repo,
            get_empty_lazy_repo, run_future, string_to_nodehash, upload_file_no_parents,
            upload_file_one_parent, upload_manifest_no_parents, upload_manifest_one_parent};

fn upload_blob_no_parents(repo: BlobRepo) {
    let expected_hash = string_to_nodehash("c3127cdbf2eae0f09653f9237d85c8436425b246");
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    // The blob does not exist...
    assert!(run_future(repo.get_file_content(&expected_hash)).is_err());

    // We upload it...
    let (hash, future) = upload_file_no_parents(&repo, "blob", &fake_path);
    assert!(hash == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future).unwrap();
    assert!(path == fake_path);
    assert!(entry.get_hash() == &HgEntryId::new(expected_hash));
    assert!(entry.get_type() == manifest::Type::File(FileType::Regular));
    assert!(
        entry.get_name() == Some(&MPathElement::new("file".into()).expect("valid MPathElement"))
    );

    let content = run_future(entry.get_content()).unwrap();
    match content {
        manifest::Content::File(FileContents::Bytes(f)) => assert_eq!(f.as_ref(), &b"blob"[..]),
        _ => panic!(),
    };

    // And the blob now exists
    let bytes = run_future(repo.get_file_content(&expected_hash)).unwrap();
    assert!(&bytes.into_bytes() == &b"blob"[..]);
}

test_both_repotypes!(
    upload_blob_no_parents,
    upload_blob_no_parents_lazy,
    upload_blob_no_parents_eager
);

fn upload_blob_one_parent(repo: BlobRepo) {
    let expected_hash = string_to_nodehash("c2d60b35a8e7e034042a9467783bbdac88a0d219");
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    let (p1, future) = upload_file_no_parents(&repo, "blob", &fake_path);

    // The blob does not exist...
    run_future(repo.get_file_content(&expected_hash)).is_err();

    // We upload it...
    let (hash, future2) = upload_file_one_parent(&repo, "blob", &fake_path, p1);
    assert!(hash == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future2.join(future).map(|(item, _)| item)).unwrap();

    assert!(path == fake_path);
    assert!(entry.get_hash() == &HgEntryId::new(expected_hash));
    assert!(entry.get_type() == manifest::Type::File(FileType::Regular));
    assert!(
        entry.get_name() == Some(&MPathElement::new("file".into()).expect("valid MPathElement"))
    );

    let content = run_future(entry.get_content()).unwrap();
    match content {
        manifest::Content::File(FileContents::Bytes(f)) => assert_eq!(f.as_ref(), &b"blob"[..]),
        _ => panic!(),
    };
    // And the blob now exists
    let bytes = run_future(repo.get_file_content(&expected_hash)).unwrap();
    assert!(&bytes.into_bytes() == &b"blob"[..]);
}

test_both_repotypes!(
    upload_blob_one_parent,
    upload_blob_one_parent_lazy,
    upload_blob_one_parent_eager
);

fn create_one_changeset(repo: BlobRepo) {
    let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");
    let expected_files = vec![
        RepoPath::file("dir/file")
            .expect("Can't generate fake RepoPath")
            .mpath()
            .unwrap()
            .clone(),
    ];
    let author: String = "author <author@fb.com>".into();

    let (filehash, file_future) = upload_file_no_parents(&repo, "blob", &fake_file_path);

    let (dirhash, manifest_dir_future) =
        upload_manifest_no_parents(&repo, format!("file\0{}\n", filehash), &fake_dir_path);

    let (roothash, root_manifest_future) =
        upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

    let commit = create_changeset_no_parents(
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future, manifest_dir_future],
    );

    let cs = run_future(commit.get_completed_changeset()).unwrap();
    assert!(cs.manifestid() == &HgManifestId::new(roothash));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));
    let files: Vec<_> = cs.files().into();
    assert!(
        files == expected_files,
        format!("Got {:?}, expected {:?}", files, expected_files)
    );

    // And check the file blob is present
    let bytes = run_future(repo.get_file_content(&filehash)).unwrap();
    assert!(&bytes.into_bytes() == &b"blob"[..]);
}

test_both_repotypes!(
    create_one_changeset,
    create_one_changeset_lazy,
    create_one_changeset_eager
);

fn create_two_changesets(repo: BlobRepo) {
    let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::file("dir").expect("Can't generate fake RepoPath");
    let utf_author: String = "\u{041F}\u{0451}\u{0442}\u{0440} <peter@fb.com>".into();

    let (filehash, file_future) = upload_file_no_parents(&repo, "blob", &fake_file_path);

    let (dirhash, manifest_dir_future) =
        upload_manifest_no_parents(&repo, format!("file\0{}\n", filehash), &fake_dir_path);

    let (roothash, root_manifest_future) =
        upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

    let commit1 = create_changeset_no_parents(
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future, manifest_dir_future],
    );

    let (roothash, root_manifest_future) = upload_manifest_one_parent(
        &repo,
        format!("file\0{}\n", filehash),
        &RepoPath::root(),
        roothash,
    );

    let commit2 = create_changeset_one_parent(
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![],
        commit1.clone(),
    );

    let (commit1, commit2) = run_future(
        commit1
            .get_completed_changeset()
            .join(commit2.get_completed_changeset()),
    ).unwrap();

    assert!(commit2.manifestid() == &HgManifestId::new(roothash));
    assert!(commit2.user() == utf_author.as_bytes());
    let files: Vec<_> = commit2.files().into();
    let expected_files = vec![MPath::new("dir/file").unwrap(), MPath::new("file").unwrap()];
    assert!(
        files == expected_files,
        format!("Got {:?}, expected {:?}", files, expected_files)
    );

    assert!(commit1.parents().get_nodes() == (None, None));
    let commit1_id = Some(commit1.get_changeset_id().into_nodehash());
    let expected_parents = (commit1_id.as_ref(), None);
    assert!(commit2.parents().get_nodes() == expected_parents);

    let linknode = run_future(repo.get_linknode(fake_file_path, &filehash)).unwrap();
    assert!(
        linknode == commit1.get_changeset_id(),
        "Bad linknode {} - should be {}",
        linknode,
        commit1.get_changeset_id()
    );
}

test_both_repotypes!(
    create_two_changesets,
    create_two_changesets_lazy,
    create_two_changesets_eager
);

fn create_bad_changeset(repo: BlobRepo) {
    let dirhash = string_to_nodehash("c2d60b35a8e7e034042a9467783bbdac88a0d219");

    let (_, root_manifest_future) =
        upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

    let commit =
        create_changeset_no_parents(&repo, root_manifest_future.map(Some).boxify(), vec![]);

    run_future(commit.get_completed_changeset()).unwrap();
}

test_both_repotypes!(
    should_panic,
    create_bad_changeset,
    create_bad_changeset_lazy,
    create_bad_changeset_eager
);

fn create_double_linknode(repo: BlobRepo) {
    let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");

    let (filehash, parent_commit) = {
        let (filehash, file_future) = upload_file_no_parents(&repo, "blob", &fake_file_path);
        let (dirhash, manifest_dir_future) =
            upload_manifest_no_parents(&repo, format!("file\0{}\n", filehash), &fake_dir_path);
        let (_, root_manifest_future) =
            upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

        (
            filehash,
            create_changeset_no_parents(
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![manifest_dir_future, file_future],
            ),
        )
    };

    let child_commit = {
        let (filehash, file_future) =
            upload_file_one_parent(&repo, "blob", &fake_file_path, filehash);

        let (dirhash, manifest_dir_future) =
            upload_manifest_no_parents(&repo, format!("file\0{}\n", filehash), &fake_dir_path);

        let (_, root_manifest_future) =
            upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

        create_changeset_one_parent(
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![manifest_dir_future, file_future],
            parent_commit.clone(),
        )
    };
    let child = run_future(child_commit.get_completed_changeset()).unwrap();
    let parent = run_future(parent_commit.get_completed_changeset()).unwrap();

    let linknode = run_future(repo.get_linknode(fake_file_path, &filehash)).unwrap();
    assert!(
        linknode != child.get_changeset_id(),
        "Linknode on child commit = should be on parent"
    );
    assert!(
        linknode == parent.get_changeset_id(),
        "Linknode not on parent commit - ended up on {} instead",
        linknode
    );
}

test_both_repotypes!(
    create_double_linknode,
    create_double_linknode_lazy,
    create_double_linknode_eager
);

fn check_linknode_creation(repo: BlobRepo) {
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");
    let author: String = "author <author@fb.com>".into();

    let files: Vec<_> = (1..100)
        .into_iter()
        .map(|id| {
            let path = RepoPath::file(
                MPath::new(format!("file{}", id)).expect("String to MPath failed"),
            ).expect("Can't generate fake RepoPath");
            let (hash, future) = upload_file_no_parents(&repo, format!("blob id {}", id), &path);
            ((hash, path), future)
        })
        .collect();

    let (metadata, mut uploads): (Vec<_>, Vec<_>) = files.into_iter().unzip();

    let manifest = metadata
        .iter()
        .fold(String::new(), |mut acc, &(hash, ref path)| {
            acc.push_str(format!("{}\0{}\n", path, hash).as_str());
            acc
        });

    let (dirhash, manifest_dir_future) =
        upload_manifest_no_parents(&repo, manifest, &fake_dir_path);

    let (roothash, root_manifest_future) =
        upload_manifest_no_parents(&repo, format!("dir\0{}t\n", dirhash), &RepoPath::root());

    uploads.push(manifest_dir_future);

    let commit =
        create_changeset_no_parents(&repo, root_manifest_future.map(Some).boxify(), uploads);

    let cs = run_future(commit.get_completed_changeset()).unwrap();
    assert!(cs.manifestid() == &HgManifestId::new(roothash));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));

    let cs_id = cs.get_changeset_id();
    // And check all the linknodes got created
    metadata.into_iter().for_each(|(hash, path)| {
        let linknode = run_future(repo.get_linknode(path, &hash)).unwrap();
        assert!(
            linknode == cs_id,
            "Linknode is {}, should be {}",
            linknode,
            cs_id
        );
    })
}

test_both_repotypes!(
    check_linknode_creation,
    check_linknode_creation_lazy,
    check_linknode_creation_eager
);

struct StoreFetchTestable<K> {
    repo: BlobRepo,
    _key: PhantomData<K>,
}

impl<K> StoreFetchTestable<K> {
    fn new(repo: &BlobRepo) -> Self {
        StoreFetchTestable {
            repo: repo.clone(),
            _key: PhantomData,
        }
    }
}

impl<K> Testable for StoreFetchTestable<K>
where
    K: MononokeId,
    K::Value: PartialEq + Arbitrary,
{
    fn result<G: Gen>(&self, g: &mut G) -> TestResult {
        let value = <K::Value as Arbitrary>::arbitrary(g);
        let value_cloned = value.clone();
        let store_fetch_future = self.repo
            .unittest_store(value)
            .and_then({
                let repo = self.repo.clone();
                move |key| repo.unittest_fetch(&key)
            })
            .map(move |value_fetched| TestResult::from_bool(value_fetched == value_cloned));
        run_future(store_fetch_future).expect("valid mononoke type")
    }
}

fn store_fetch_mononoke_types(repo: BlobRepo) {
    quickcheck(StoreFetchTestable::<ChangesetId>::new(&repo));
    quickcheck(StoreFetchTestable::<ContentId>::new(&repo));
}

test_both_repotypes!(
    store_fetch_mononoke_types,
    store_fetch_mononoke_types_lazy,
    store_fetch_mononoke_types_eager
);

#[test]
fn test_compute_changed_files_no_parents() {
    async_unit::tokio_unit_test(|| {
        let repo = many_files_dirs::getrepo(None);
        let nodehash = string_to_nodehash("a6cb7dddec32acaf9a28db46cdb3061682155531");
        let expected = vec![
            MPath::new(b"1").unwrap(),
            MPath::new(b"2").unwrap(),
            MPath::new(b"dir1").unwrap(),
            MPath::new(b"dir2/file_1_in_dir2").unwrap(),
        ];

        let cs =
            run_future(repo.get_changeset_by_changesetid(&HgChangesetId::new(nodehash))).unwrap();
        let mf = run_future(repo.get_manifest_by_nodeid(&cs.manifestid().into_nodehash())).unwrap();

        let diff = run_future(compute_changed_files(&mf, None, None)).unwrap();
        assert!(
            diff == expected,
            "Got {:?}, expected {:?}\n",
            diff,
            expected,
        );
    });
}

#[test]
fn test_compute_changed_files_one_parent() {
    async_unit::tokio_unit_test(|| {
        // Note that this is a commit and its parent commit, so you can use:
        // hg log -T"{node}\n{files % '    MPath::new(b\"{file}\").unwrap(),\\n'}\\n" -r $HASH
        // to see how Mercurial would compute the files list and confirm that it's the same
        let repo = many_files_dirs::getrepo(None);
        let nodehash = string_to_nodehash("a6cb7dddec32acaf9a28db46cdb3061682155531");
        let parenthash = string_to_nodehash("473b2e715e0df6b2316010908879a3c78e275dd9");
        let expected = vec![
            MPath::new(b"dir1").unwrap(),
            MPath::new(b"dir1/file_1_in_dir1").unwrap(),
            MPath::new(b"dir1/file_2_in_dir1").unwrap(),
            MPath::new(b"dir1/subdir1/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir1/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir2/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir2/file_2").unwrap(),
        ];

        let cs =
            run_future(repo.get_changeset_by_changesetid(&HgChangesetId::new(nodehash))).unwrap();
        let mf = run_future(repo.get_manifest_by_nodeid(&cs.manifestid().into_nodehash())).unwrap();

        let parent_cs =
            run_future(repo.get_changeset_by_changesetid(&HgChangesetId::new(parenthash))).unwrap();
        let parent_mf = run_future(repo.get_manifest_by_nodeid(
            &parent_cs.manifestid().into_nodehash(),
        )).unwrap();

        let diff = run_future(compute_changed_files(&mf, Some(&parent_mf), None)).unwrap();
        assert!(
            diff == expected,
            "Got {:?}, expected {:?}\n",
            diff,
            expected,
        );
    });
}

fn make_bonsai_changeset(
    p0: Option<ChangesetId>,
    p1: Option<ChangesetId>,
    changes: Vec<(&'static str, Option<FileChange>)>,
) -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents: p0.into_iter().chain(p1).collect(),
        author: "aslpavel".to_owned(),
        author_date: DateTime::from_timestamp(1528298184, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "[mononoke] awesome message".to_owned(),
        extra: BTreeMap::new(),
        file_changes: changes
            .into_iter()
            .map(|(path, change)| (MPath::new(path).unwrap(), change))
            .collect(),
    }.freeze()
        .unwrap()
}

fn make_file_change(
    content: impl AsRef<[u8]>,
    repo: &BlobRepo,
) -> impl Future<Item = FileChange, Error = Error> + Send {
    let content = content.as_ref();
    let content_size = content.len() as u64;
    repo.unittest_store(FileContents::new_bytes(content.as_ref()))
        .map(move |content_id| FileChange::new(content_id, FileType::Regular, content_size, None))
}

#[test]
fn test_get_manifest_from_bonsai() {
    async_unit::tokio_unit_test(|| {
        let repo = merge_uneven::getrepo(None);
        let get_manifest_for_changeset =
            |cs_nodehash: &str| -> HgNodeHash {
                run_future(repo.get_changeset_by_changesetid(&HgChangesetId::new(
                    string_to_nodehash(cs_nodehash),
                ))).unwrap()
                    .manifestid()
                    .into_nodehash()
            };
        let get_entries =
            |ms_hash: &HgNodeHash| -> BoxFuture<HashMap<String, Box<Entry + Sync>>, Error> {
                repo.get_manifest_by_nodeid(ms_hash)
                    .map(|ms| {
                        ms.list()
                            .map(|e| {
                                let name = e.get_name().unwrap().as_bytes().to_owned();
                                (String::from_utf8(name).unwrap(), e)
                            })
                            .collect::<HashMap<_, _>>()
                    })
                    .boxify()
            };

        // #CONTENT
        // 1: 1
        // 2: 2
        // 3: 3
        // 4: 4
        // 5: 5
        // base: branch1
        // branch: 4
        let ms1 = get_manifest_for_changeset("264f01429683b3dd8042cb3979e8bf37007118bc");

        // #CONTENT
        // base: base
        // branch: 4
        let ms2 = get_manifest_for_changeset("16839021e338500b3cf7c9b871c8a07351697d68");

        // fails with conflict
        {
            let ms_hash = run_future(repo.get_manifest_from_bonsai(
                make_bonsai_changeset(None, None, vec![]),
                Some(&ms1),
                Some(&ms2),
            ));
            assert!(match ms_hash
                .expect_err("should have failed")
                .downcast::<ErrorKind>()
                .unwrap()
            {
                ErrorKind::UnresolvedConflicts => true,
                _ => false,
            });
        }

        // resolves same content different parents for `branch` file
        {
            let ms_hash = run_future(repo.get_manifest_from_bonsai(
                make_bonsai_changeset(None, None, vec![("base", None)]),
                Some(&ms1),
                Some(&ms2),
            )).expect("merge should have succeeded");
            let entries = run_future(get_entries(&ms_hash)).unwrap();

            assert!(entries.get("1").is_some());
            assert!(entries.get("2").is_some());
            assert!(entries.get("3").is_some());
            assert!(entries.get("4").is_some());
            assert!(entries.get("5").is_some());
            assert!(entries.get("base").is_none());

            // check trivial merge parents
            let (ms1_entries, ms2_entries) =
                run_future(get_entries(&ms1).join(get_entries(&ms2))).unwrap();
            let mut br_expected_parents = HashSet::new();
            br_expected_parents.insert(
                ms1_entries
                    .get("branch")
                    .unwrap()
                    .get_hash()
                    .into_nodehash(),
            );
            br_expected_parents.insert(
                ms2_entries
                    .get("branch")
                    .unwrap()
                    .get_hash()
                    .into_nodehash(),
            );

            let br = entries.get("branch").expect("trivial merge should succeed");
            let br_parents = run_future(br.get_parents())
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>();
            assert_eq!(br_parents, br_expected_parents);
        }

        // add file
        {
            let content_expected = &b"some awesome content"[..];
            let fc = run_future(make_file_change(content_expected, &repo)).unwrap();
            let bcs = make_bonsai_changeset(None, None, vec![("base", None), ("new", Some(fc))]);
            let ms_hash = run_future(repo.get_manifest_from_bonsai(bcs, Some(&ms1), Some(&ms2)))
                .expect("adding new file should not produce coflict");
            let entries = run_future(get_entries(&ms_hash)).unwrap();
            let new = entries.get("new").expect("new file should be in entries");
            match run_future(new.get_content()).unwrap() {
                manifest::Content::File(content) => {
                    assert_eq!(content, FileContents::new_bytes(content_expected));
                }
                _ => panic!("content type mismatch"),
            };
            let new_parents = run_future(new.get_parents()).unwrap();
            assert_eq!(new_parents, HgParents::None);
        }
    });
}
