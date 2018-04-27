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
#[macro_use]
extern crate maplit;
#[macro_use]
extern crate slog;

extern crate blobrepo;
extern crate changesets;
extern crate dbbookmarks;
extern crate many_files_dirs;
extern crate memblob;
extern crate mercurial;
extern crate mercurial_types;
extern crate mononoke_types;

use futures::Future;
use futures_ext::FutureExt;

use blobrepo::{compute_changed_files, BlobRepo};
use mercurial_types::{manifest, Changeset, DChangesetId, DEntryId, DManifestId, Entry, FileType,
                      MPath, MPathElement, RepoPath};
use mononoke_types::FileContents;

mod stats_units;
#[macro_use]
mod utils;

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
    assert!(hash.into_mononoke() == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future).unwrap();
    assert!(path == fake_path);
    assert!(entry.get_hash() == &DEntryId::new(expected_hash));
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
    assert!(hash.into_mononoke() == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future2.join(future).map(|(item, _)| item)).unwrap();

    assert!(path == fake_path);
    assert!(entry.get_hash() == &DEntryId::new(expected_hash));
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
    assert!(cs.manifestid() == &DManifestId::new(roothash.into_mononoke()));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));
    let files: Vec<_> = cs.files().into();
    assert!(
        files == expected_files,
        format!("Got {:?}, expected {:?}", files, expected_files)
    );

    // And check the file blob is present
    let bytes = run_future(repo.get_file_content(&filehash.into_mononoke())).unwrap();
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

    assert!(commit2.manifestid() == &DManifestId::new(roothash.into_mononoke()));
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

    let linknode =
        run_future(repo.get_linknode(fake_file_path, &filehash.into_mononoke())).unwrap();
    assert!(
        linknode == commit1.get_changeset_id().into_nodehash(),
        "Bad linknode {} - should be {}",
        linknode,
        commit1.get_changeset_id().into_nodehash()
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
        let (filehash, file_future) = upload_file_no_parents(&repo, "blob", &fake_file_path);

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

    let linknode =
        run_future(repo.get_linknode(fake_file_path, &filehash.into_mononoke())).unwrap();
    assert!(
        linknode != child.get_changeset_id().into_nodehash(),
        "Linknode on child commit = should be on parent"
    );
    assert!(
        linknode == parent.get_changeset_id().into_nodehash(),
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
    assert!(cs.manifestid() == &DManifestId::new(roothash.into_mononoke()));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));

    let cs_id = cs.get_changeset_id().into_nodehash();
    // And check all the linknodes got created
    metadata.into_iter().for_each(|(hash, path)| {
        let linknode = run_future(repo.get_linknode(path, &hash.into_mononoke())).unwrap();
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
            run_future(repo.get_changeset_by_changesetid(&DChangesetId::new(nodehash))).unwrap();
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
            run_future(repo.get_changeset_by_changesetid(&DChangesetId::new(nodehash))).unwrap();
        let mf = run_future(repo.get_manifest_by_nodeid(&cs.manifestid().into_nodehash())).unwrap();

        let parent_cs =
            run_future(repo.get_changeset_by_changesetid(&DChangesetId::new(parenthash))).unwrap();
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
