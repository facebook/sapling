// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

mod tracing_blobstore;
mod utils;

use benchmark_lib::{new_benchmark_repo, DelaySettings, GenManifest};
use blobrepo::{compute_changed_files, BlobRepo};
use blobstore::Storable;
use cloned::cloned;
use context::CoreContext;
use failure_ext::Error;
use fbinit::FacebookInit;
use fixtures::{create_bonsai_changeset, many_files_dirs, merge_uneven};
use futures::{Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use maplit::btreemap;
use memblob::LazyMemblob;
use mercurial_types::{
    blobs::{ContentBlobMeta, File, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash},
    manifest, Changeset, FileType, HgChangesetId, HgEntry, HgFileNodeId, HgManifestId, HgParents,
    MPath, MPathElement, RepoPath,
};
use mercurial_types_mocks::nodehash::ONES_FNID;
use mononoke_types::bonsai_changeset::BonsaiChangesetMut;
use mononoke_types::{
    blob::BlobstoreValue, BonsaiChangeset, ChangesetId, DateTime, FileChange, FileContents,
};
use rand::{distributions::Normal, SeedableRng};
use rand_xorshift::XorShiftRng;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    iter::FromIterator,
    sync::Arc,
};
use tests_utils::{create_commit, store_files};
use tokio::runtime::Runtime;
use tracing_blobstore::TracingBlobstore;
use utils::{
    create_changeset_no_parents, create_changeset_one_parent, get_empty_eager_repo,
    get_empty_lazy_repo, run_future, string_to_nodehash, to_mpath, upload_file_no_parents,
    upload_file_one_parent, upload_manifest_no_parents, upload_manifest_one_parent,
};

fn upload_blob_no_parents(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let expected_hash = HgFileNodeId::new(string_to_nodehash(
        "c3127cdbf2eae0f09653f9237d85c8436425b246",
    ));
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    // The blob does not exist...
    assert!(run_future(repo.get_file_content(ctx.clone(), expected_hash).concat2()).is_err());

    // We upload it...
    let (hash, future) = upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_path);
    assert!(hash == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future).unwrap();
    assert!(path == fake_path);
    assert!(HgFileNodeId::new(entry.get_hash().into_nodehash()) == expected_hash);
    assert!(entry.get_type() == manifest::Type::File(FileType::Regular));
    assert!(
        entry.get_name() == Some(&MPathElement::new("file".into()).expect("valid MPathElement"))
    );

    let content = run_future(entry.get_content(ctx.clone())).unwrap();
    let stream = match content {
        manifest::Content::File(stream) => stream,
        _ => panic!(),
    };
    let bytes = run_future(stream.concat2()).unwrap();
    assert_eq!(bytes.into_bytes().as_ref(), &b"blob"[..]);

    // And the blob now exists
    let bytes = run_future(repo.get_file_content(ctx.clone(), expected_hash).concat2()).unwrap();
    assert!(bytes.into_bytes().as_ref() == &b"blob"[..]);
}

test_both_repotypes!(
    upload_blob_no_parents,
    upload_blob_no_parents_lazy,
    upload_blob_no_parents_eager
);

fn upload_blob_one_parent(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let expected_hash = HgFileNodeId::new(string_to_nodehash(
        "c2d60b35a8e7e034042a9467783bbdac88a0d219",
    ));
    let fake_path = RepoPath::file("fake/file").expect("Can't generate fake RepoPath");

    let (p1, future) = upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_path);

    // The blob does not exist...
    let _ = run_future(repo.get_file_content(ctx.clone(), expected_hash).concat2()).unwrap_err();

    // We upload it...
    let (hash, future2) = upload_file_one_parent(ctx.clone(), &repo, "blob", &fake_path, p1);
    assert!(hash == expected_hash);

    // The entry we're given is correct...
    let (entry, path) = run_future(future2.join(future).map(|(item, _)| item)).unwrap();

    assert!(path == fake_path);
    assert!(HgFileNodeId::new(entry.get_hash().into_nodehash()) == expected_hash);
    assert!(entry.get_type() == manifest::Type::File(FileType::Regular));
    assert!(
        entry.get_name() == Some(&MPathElement::new("file".into()).expect("valid MPathElement"))
    );

    let content = run_future(entry.get_content(ctx.clone())).unwrap();
    let stream = match content {
        manifest::Content::File(stream) => stream,
        _ => panic!(),
    };
    let bytes = run_future(stream.concat2()).unwrap();
    assert_eq!(bytes.into_bytes().as_ref(), &b"blob"[..]);

    // And the blob now exists
    let bytes = run_future(repo.get_file_content(ctx.clone(), expected_hash).concat2()).unwrap();
    assert!(bytes.into_bytes().as_ref() == &b"blob"[..]);
}

test_both_repotypes!(
    upload_blob_one_parent,
    upload_blob_one_parent_lazy,
    upload_blob_one_parent_eager
);

fn create_one_changeset(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let fake_file_path = RepoPath::file("dir/file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");
    let expected_files = vec![RepoPath::file("dir/file")
        .expect("Can't generate fake RepoPath")
        .mpath()
        .unwrap()
        .clone()];
    let author: String = "author <author@fb.com>".into();

    let (filehash, file_future) =
        upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

    let (dirhash, manifest_dir_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("file\0{}\n", filehash),
        &fake_dir_path,
    );

    let (roothash, root_manifest_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("dir\0{}t\n", dirhash),
        &RepoPath::root(),
    );

    let commit = create_changeset_no_parents(
        fb,
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future, manifest_dir_future],
    );

    let bonsai_hg = run_future(commit.get_completed_changeset()).unwrap();
    let cs = &bonsai_hg.1;
    assert!(cs.manifestid() == HgManifestId::new(roothash));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));
    let files: Vec<_> = cs.files().into();
    assert!(
        files == expected_files,
        format!("Got {:?}, expected {:?}", files, expected_files)
    );

    // And check the file blob is present
    let bytes = run_future(repo.get_file_content(ctx.clone(), filehash).concat2()).unwrap();
    assert!(bytes.into_bytes().as_ref() == &b"blob"[..]);
}

test_both_repotypes!(
    create_one_changeset,
    create_one_changeset_lazy,
    create_one_changeset_eager
);

fn create_two_changesets(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let fake_file_path = RepoPath::file("dir/file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");
    let utf_author: String = "\u{041F}\u{0451}\u{0442}\u{0440} <peter@fb.com>".into();

    let (filehash, file_future) =
        upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

    let (dirhash, manifest_dir_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("file\0{}\n", filehash),
        &fake_dir_path,
    );

    let (roothash, root_manifest_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("dir\0{}t\n", dirhash),
        &RepoPath::root(),
    );

    let commit1 = create_changeset_no_parents(
        fb,
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future, manifest_dir_future],
    );

    let fake_file_path_no_dir = RepoPath::file("file").expect("Can't generate fake RepoPath");
    let (filehash, file_future) =
        upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_no_dir);
    let (roothash, root_manifest_future) = upload_manifest_one_parent(
        ctx.clone(),
        &repo,
        format!("file\0{}\n", filehash),
        &RepoPath::root(),
        roothash,
    );

    let commit2 = create_changeset_one_parent(
        fb,
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future],
        commit1.clone(),
    );

    let (commit1, commit2) = run_future(
        commit1
            .get_completed_changeset()
            .join(commit2.get_completed_changeset()),
    )
    .unwrap();

    let commit1 = &commit1.1;
    let commit2 = &commit2.1;
    assert!(commit2.manifestid() == HgManifestId::new(roothash));
    assert!(commit2.user() == utf_author.as_bytes());
    let files: Vec<_> = commit2.files().into();
    let expected_files = vec![MPath::new("dir/file").unwrap(), MPath::new("file").unwrap()];
    assert!(
        files == expected_files,
        format!("Got {:?}, expected {:?}", files, expected_files)
    );

    assert!(commit1.parents().get_nodes() == (None, None));
    let commit1_id = Some(commit1.get_changeset_id().into_nodehash());
    let expected_parents = (commit1_id, None);
    assert!(commit2.parents().get_nodes() == expected_parents);

    let linknode = run_future(repo.get_linknode(ctx, &fake_file_path, filehash)).unwrap();
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

fn check_bonsai_creation(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let fake_file_path = RepoPath::file("dir/file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");

    let (filehash, file_future) =
        upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

    let (dirhash, manifest_dir_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("file\0{}\n", filehash),
        &fake_dir_path,
    );

    let (_, root_manifest_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("dir\0{}t\n", dirhash),
        &RepoPath::root(),
    );

    let commit = create_changeset_no_parents(
        fb,
        &repo,
        root_manifest_future.map(Some).boxify(),
        vec![file_future, manifest_dir_future],
    );

    let commit = run_future(commit.get_completed_changeset()).unwrap();
    let commit = &commit.1;
    let bonsai_cs_id =
        run_future(repo.get_bonsai_from_hg(ctx.clone(), commit.get_changeset_id())).unwrap();
    assert!(bonsai_cs_id.is_some());
    let bonsai = run_future(repo.get_bonsai_changeset(ctx.clone(), bonsai_cs_id.unwrap())).unwrap();
    assert_eq!(
        bonsai
            .file_changes()
            .map(|fc| format!("{}", fc.0))
            .collect::<Vec<_>>(),
        vec![String::from("dir/file")]
    );
}

test_both_repotypes!(
    check_bonsai_creation,
    check_bonsai_creation_lazy,
    check_bonsai_creation_eager
);

fn check_bonsai_creation_with_rename(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let parent = {
        let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");

        let (filehash, file_future) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

        let (_, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\n", filehash),
            &RepoPath::root(),
        );

        create_changeset_no_parents(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future],
        )
    };

    let child = {
        let fake_renamed_file_path =
            RepoPath::file("file_rename").expect("Can't generate fake RepoPath");

        let (filehash, file_future) = upload_file_no_parents(
            ctx.clone(),
            &repo,
            "\x01\ncopy: file\ncopyrev: c3127cdbf2eae0f09653f9237d85c8436425b246\x01\nblob",
            &fake_renamed_file_path,
        );

        let (_, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file_rename\0{}\n", filehash),
            &RepoPath::root(),
        );

        create_changeset_one_parent(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future],
            parent.clone(),
        )
    };

    let parent_cs = run_future(parent.get_completed_changeset()).unwrap();
    let parent_cs = &parent_cs.1;
    let child_cs = run_future(child.get_completed_changeset()).unwrap();
    let child_cs = &child_cs.1;

    let parent_bonsai_cs_id =
        run_future(repo.get_bonsai_from_hg(ctx.clone(), parent_cs.get_changeset_id()))
            .unwrap()
            .unwrap();

    let bonsai_cs_id =
        run_future(repo.get_bonsai_from_hg(ctx.clone(), child_cs.get_changeset_id())).unwrap();
    let bonsai = run_future(repo.get_bonsai_changeset(ctx.clone(), bonsai_cs_id.unwrap())).unwrap();
    let fc = bonsai.file_changes().collect::<BTreeMap<_, _>>();
    let file = MPath::new("file").unwrap();
    assert!(!fc[&file].is_some());
    let file_rename = MPath::new("file_rename").unwrap();
    assert!(fc[&file_rename].is_some());
    assert_eq!(
        fc[&file_rename].unwrap().copy_from(),
        Some(&(file, parent_bonsai_cs_id))
    );
}

test_both_repotypes!(
    check_bonsai_creation_with_rename,
    check_bonsai_creation_with_rename_lazy,
    check_bonsai_creation_with_rename_eager
);

fn create_bad_changeset(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let dirhash = string_to_nodehash("c2d60b35a8e7e034042a9467783bbdac88a0d219");

    let (_, root_manifest_future) = upload_manifest_no_parents(
        ctx,
        &repo,
        format!("dir\0{}t\n", dirhash),
        &RepoPath::root(),
    );

    let commit =
        create_changeset_no_parents(fb, &repo, root_manifest_future.map(Some).boxify(), vec![]);

    run_future(commit.get_completed_changeset()).unwrap();
}

test_both_repotypes!(
    should_panic,
    create_bad_changeset,
    create_bad_changeset_lazy,
    create_bad_changeset_eager
);

fn create_double_linknode(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let fake_file_path = RepoPath::file("dir/file").expect("Can't generate fake RepoPath");
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");

    let (filehash, parent_commit) = {
        let (filehash, file_future) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);
        let (dirhash, manifest_dir_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\n", filehash),
            &fake_dir_path,
        );
        let (_, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("dir\0{}t\n", dirhash),
            &RepoPath::root(),
        );

        (
            filehash,
            create_changeset_no_parents(
                fb,
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![manifest_dir_future, file_future],
            ),
        )
    };

    let child_commit = {
        let (filehash, file_future) =
            upload_file_one_parent(ctx.clone(), &repo, "blob", &fake_file_path, filehash);

        let (dirhash, manifest_dir_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\n", filehash),
            &fake_dir_path,
        );

        let (_, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("dir\0{}t\n", dirhash),
            &RepoPath::root(),
        );

        create_changeset_one_parent(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![manifest_dir_future, file_future],
            parent_commit.clone(),
        )
    };
    let child = run_future(child_commit.get_completed_changeset()).unwrap();
    let child = &child.1;
    let parent = run_future(parent_commit.get_completed_changeset()).unwrap();
    let parent = &parent.1;

    let linknode = run_future(repo.get_linknode(ctx, &fake_file_path, filehash)).unwrap();
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

fn check_linknode_creation(fb: FacebookInit, repo: BlobRepo) {
    let ctx = CoreContext::test_mock(fb);
    let fake_dir_path = RepoPath::dir("dir").expect("Can't generate fake RepoPath");
    let author: String = "author <author@fb.com>".into();

    let files: Vec<_> = (1..100)
        .into_iter()
        .map(|id| {
            let path = RepoPath::file(
                MPath::new(format!("dir/file{}", id)).expect("String to MPath failed"),
            )
            .expect("Can't generate fake RepoPath");
            let (hash, future) =
                upload_file_no_parents(ctx.clone(), &repo, format!("blob id {}", id), &path);
            ((hash, format!("file{}", id)), future)
        })
        .collect();

    let (metadata, mut uploads): (Vec<_>, Vec<_>) = files.into_iter().unzip();

    let manifest = metadata
        .iter()
        .fold(String::new(), |mut acc, &(hash, ref basename)| {
            acc.push_str(format!("{}\0{}\n", basename, hash).as_str());
            acc
        });

    let (dirhash, manifest_dir_future) =
        upload_manifest_no_parents(ctx.clone(), &repo, manifest, &fake_dir_path);

    let (roothash, root_manifest_future) = upload_manifest_no_parents(
        ctx.clone(),
        &repo,
        format!("dir\0{}t\n", dirhash),
        &RepoPath::root(),
    );

    uploads.push(manifest_dir_future);

    let commit =
        create_changeset_no_parents(fb, &repo, root_manifest_future.map(Some).boxify(), uploads);

    let cs = run_future(commit.get_completed_changeset()).unwrap();
    let cs = &cs.1;
    assert!(cs.manifestid() == HgManifestId::new(roothash));
    assert!(cs.user() == author.as_bytes());
    assert!(cs.parents().get_nodes() == (None, None));

    let cs_id = cs.get_changeset_id();
    // And check all the linknodes got created
    metadata.into_iter().for_each(|(hash, basename)| {
        let path = RepoPath::file(format!("dir/{}", basename).as_str())
            .expect("Can't generate fake RepoPath");
        let linknode = run_future(repo.get_linknode(ctx.clone(), &path, hash)).unwrap();
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

#[fbinit::test]
fn test_compute_changed_files_no_parents(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = many_files_dirs::getrepo(fb);
        let nodehash = string_to_nodehash("051946ed218061e925fb120dac02634f9ad40ae2");
        let expected = vec![
            MPath::new(b"1").unwrap(),
            MPath::new(b"2").unwrap(),
            MPath::new(b"dir1").unwrap(),
            MPath::new(b"dir2/file_1_in_dir2").unwrap(),
        ];

        let cs = run_future(
            repo.get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(nodehash)),
        )
        .unwrap();

        let diff = run_future(compute_changed_files(
            ctx.clone(),
            repo.clone(),
            cs.manifestid(),
            None,
            None,
        ))
        .unwrap();
        assert!(
            diff == expected,
            "Got {:?}, expected {:?}\n",
            diff,
            expected,
        );
    });
}

#[fbinit::test]
fn test_compute_changed_files_one_parent(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        // Note that this is a commit and its parent commit, so you can use:
        // hg log -T"{node}\n{files % '    MPath::new(b\"{file}\").unwrap(),\\n'}\\n" -r $HASH
        // to see how Mercurial would compute the files list and confirm that it's the same
        let repo = many_files_dirs::getrepo(fb);
        let nodehash = string_to_nodehash("051946ed218061e925fb120dac02634f9ad40ae2");
        let parenthash = string_to_nodehash("d261bc7900818dea7c86935b3fb17a33b2e3a6b4");
        let expected = vec![
            MPath::new(b"dir1").unwrap(),
            MPath::new(b"dir1/file_1_in_dir1").unwrap(),
            MPath::new(b"dir1/file_2_in_dir1").unwrap(),
            MPath::new(b"dir1/subdir1/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir1/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir2/file_1").unwrap(),
            MPath::new(b"dir1/subdir1/subsubdir2/file_2").unwrap(),
        ];

        let cs = run_future(
            repo.get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(nodehash)),
        )
        .unwrap();

        let parent_cs = run_future(
            repo.get_changeset_by_changesetid(ctx.clone(), HgChangesetId::new(parenthash)),
        )
        .unwrap();

        let diff = run_future(compute_changed_files(
            ctx.clone(),
            repo.clone(),
            cs.manifestid(),
            Some(&parent_cs.manifestid()),
            None,
        ))
        .unwrap();
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
    }
    .freeze()
    .unwrap()
}

fn make_file_change(
    ctx: CoreContext,
    content: impl AsRef<[u8]>,
    repo: &BlobRepo,
) -> impl Future<Item = FileChange, Error = Error> + Send {
    let content = content.as_ref();
    let content_size = content.len() as u64;
    FileContents::new_bytes(content.as_ref())
        .into_blob()
        .store(ctx, &repo.get_blobstore())
        .map(move |content_id| FileChange::new(content_id, FileType::Regular, content_size, None))
}

#[fbinit::test]
fn test_find_files_in_manifest(fb: FacebookInit) -> Result<(), Error> {
    let make_paths =
        |paths: &[&str]| -> Result<HashSet<_>, _> { paths.into_iter().map(MPath::new).collect() };

    let mut rt = Runtime::new()?;
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb);

    let mf = rt
        .block_on(repo.get_changeset_by_changesetid(
            ctx.clone(),
            HgChangesetId::new(string_to_nodehash(
                "d261bc7900818dea7c86935b3fb17a33b2e3a6b4",
            )),
        ))?
        .manifestid();
    let paths = make_paths(&[
        "1",
        "1/bla/bla",
        "3",
        "dir3/bla/bla",
        "dir1/subdir1/subsubdir1/file_1",
        "dir1/subdir1/subsubdir2/file_1",
        "dir1/subdir1/subsubdir2/file_2",
        "dir1/subdir1",
    ])?;
    let files = rt.block_on(repo.find_files_in_manifest(ctx, mf, paths))?;
    assert_eq!(
        HashSet::from_iter(files.keys().cloned()),
        make_paths(&[
            "1",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2/file_1",
            "dir1/subdir1/subsubdir2/file_2",
        ])?
    );
    Ok(())
}

#[fbinit::test]
fn test_find_all_path_component_entries(fb: FacebookInit) -> Result<(), Error> {
    let make_paths =
        |paths: &[&str]| -> Result<HashSet<_>, _> { paths.into_iter().map(MPath::new).collect() };

    let mut rt = Runtime::new()?;
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb);

    let mf = rt
        .block_on(repo.get_changeset_by_changesetid(
            ctx.clone(),
            HgChangesetId::new(string_to_nodehash(
                "d261bc7900818dea7c86935b3fb17a33b2e3a6b4",
            )),
        ))?
        .manifestid();
    let paths = make_paths(&[
        "dir1/subdir1/subsubdir1/file_1",
        "dir1/subdir1/subsubdir2/file_1",
        "dir1/subdir1/subsubdir2/file_2",
    ])?;
    let files = rt.block_on(repo.find_all_path_component_entries(ctx, mf, paths))?;
    assert_eq!(
        HashSet::from_iter(files.keys().cloned()),
        make_paths(&[
            "dir1",
            "dir1/subdir1",
            "dir1/subdir1/subsubdir1",
            "dir1/subdir1/subsubdir2",
            "dir1/subdir1/subsubdir1/file_1",
            "dir1/subdir1/subsubdir2/file_1",
            "dir1/subdir1/subsubdir2/file_2",
        ])?
    );
    Ok(())
}

#[fbinit::test]
fn test_get_manifest_from_bonsai(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = merge_uneven::getrepo(fb);
        let get_manifest_for_changeset = {
            cloned!(ctx, repo);
            move |cs_nodehash: &str| -> HgManifestId {
                run_future(repo.get_changeset_by_changesetid(
                    ctx.clone(),
                    HgChangesetId::new(string_to_nodehash(cs_nodehash)),
                ))
                .unwrap()
                .manifestid()
            }
        };
        let get_entries = {
            cloned!(ctx, repo);
            move |ms_hash: HgManifestId| -> BoxFuture<HashMap<String, Box<dyn HgEntry + Sync>>, Error> {
                repo.get_manifest_by_nodeid(ctx.clone(), ms_hash)
                    .map(|ms| {
                        ms.list()
                            .map(|e| {
                                let name = e.get_name().unwrap().as_ref().to_owned();
                                (String::from_utf8(name).unwrap(), e)
                            })
                            .collect::<HashMap<_, _>>()
                    })
                    .boxify()
            }
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
                ctx.clone(),
                make_bonsai_changeset(None, None, vec![]),
                Some(ms1),
                Some(ms2),
            ));
            assert!(ms_hash
                .expect_err("should have failed")
                .to_string()
                .contains("conflict"));
        }

        // resolves same content different parents for `branch` file
        {
            let (ms_hash, _) = run_future(repo.get_manifest_from_bonsai(
                ctx.clone(),
                make_bonsai_changeset(None, None, vec![("base", None)]),
                Some(ms1),
                Some(ms2),
            ))
            .expect("merge should have succeeded");
            let entries = run_future(get_entries(ms_hash)).unwrap();

            assert!(entries.get("1").is_some());
            assert!(entries.get("2").is_some());
            assert!(entries.get("3").is_some());
            assert!(entries.get("4").is_some());
            assert!(entries.get("5").is_some());
            assert!(entries.get("base").is_none());

            // check trivial merge parents
            let (ms1_entries, ms2_entries) =
                run_future(get_entries(ms1).join(get_entries(ms2))).unwrap();
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
            let br_parents = run_future(br.get_parents(ctx.clone()))
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>();
            assert_eq!(br_parents, br_expected_parents);
        }

        // add file
        {
            let content_expected = &b"some awesome content"[..];
            let fc = run_future(make_file_change(ctx.clone(), content_expected, &repo)).unwrap();
            let bcs = make_bonsai_changeset(None, None, vec![("base", None), ("new", Some(fc))]);
            let (ms_hash, _) =
                run_future(repo.get_manifest_from_bonsai(ctx.clone(), bcs, Some(ms1), Some(ms2)))
                    .expect("adding new file should not produce coflict");
            let entries = run_future(get_entries(ms_hash)).unwrap();
            let new = entries.get("new").expect("new file should be in entries");
            let stream = match run_future(new.get_content(ctx.clone())).unwrap() {
                manifest::Content::File(stream) => stream,
                _ => panic!("content type mismatch"),
            };
            let bytes = run_future(stream.concat2()).unwrap();
            assert_eq!(bytes.into_bytes().as_ref(), content_expected.as_ref());

            let new_parents = run_future(new.get_parents(ctx.clone())).unwrap();
            assert_eq!(new_parents, HgParents::None);
        }
    });
}

#[fbinit::test]
fn test_case_conflict_in_manifest(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = many_files_dirs::getrepo(fb);
        let get_manifest_for_changeset = |cs_id: HgChangesetId| -> HgManifestId {
            run_future(repo.get_changeset_by_changesetid(ctx.clone(), cs_id))
                .unwrap()
                .manifestid()
        };

        let hg_cs = HgChangesetId::new(string_to_nodehash(
            "2f866e7e549760934e31bf0420a873f65100ad63",
        ));
        let mf = get_manifest_for_changeset(hg_cs);

        let bonsai_parent = run_future(repo.get_bonsai_from_hg(ctx.clone(), hg_cs))
            .unwrap()
            .unwrap();

        for (path, result) in &[
            ("dir1/file_1_in_dir1", false),
            ("dir1/file_1_IN_dir1", true),
            ("DiR1/file_1_in_dir1", true),
            ("dir1/other_dir/file", false),
        ] {
            let bcs_id = create_commit(
                ctx.clone(),
                repo.clone(),
                vec![bonsai_parent],
                store_files(
                    ctx.clone(),
                    btreemap! {*path => Some("caseconflicttest")},
                    repo.clone(),
                ),
            );

            let child_hg_cs =
                run_future(repo.get_hg_from_bonsai_changeset(ctx.clone(), bcs_id.clone())).unwrap();
            let child_mf = get_manifest_for_changeset(child_hg_cs);
            assert_eq!(
                run_future(repo.check_case_conflict_in_manifest(
                    ctx.clone(),
                    mf,
                    child_mf,
                    MPath::new(path).unwrap()
                ))
                .unwrap(),
                *result,
                "{} expected to {} cause conflict",
                path,
                if *result { "" } else { "not" },
            );
        }
    });
}

#[fbinit::test]
fn test_case_conflict_two_changeset(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = get_empty_lazy_repo();

        let fake_file_path_1 = RepoPath::file("file").expect("Can't generate fake RepoPath");
        let (filehash_1, file_future_1) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_1);

        let (_roothash, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\n", filehash_1),
            &RepoPath::root(),
        );

        let commit1 = create_changeset_no_parents(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future_1],
        );

        let commit2 = {
            let fake_file_path_2 = RepoPath::file("FILE").expect("Can't generate fake RepoPath");
            let (filehash_2, file_future_2) =
                upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_2);
            let (_roothash, root_manifest_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("file\0{}\nFILE\0{}\n", filehash_1, filehash_2),
                &RepoPath::root(),
            );

            create_changeset_one_parent(
                fb,
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![file_future_2],
                commit1.clone(),
            )
        };

        assert!(run_future(
            commit1
                .get_completed_changeset()
                .join(commit2.get_completed_changeset()),
        )
        .is_err());
    });
}

#[fbinit::test]
fn test_case_conflict_inside_one_changeset(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = get_empty_lazy_repo();
        let fake_file_path_1 = RepoPath::file("file").expect("Can't generate fake RepoPath");
        let (filehash_1, file_future_1) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_1);

        let fake_file_path_1 = RepoPath::file("FILE").expect("Can't generate fake RepoPath");
        let (filehash_2, file_future_2) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_1);

        let (_roothash, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\nFILE\0{}", filehash_1, filehash_2),
            &RepoPath::root(),
        );

        let commit1 = create_changeset_no_parents(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future_1, file_future_2],
        );

        assert!(run_future(commit1.get_completed_changeset()).is_err());
    });
}

#[fbinit::test]
fn test_no_case_conflict_removal(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = get_empty_lazy_repo();

        let fake_file_path_1 = RepoPath::file("file").expect("Can't generate fake RepoPath");
        let (filehash_1, file_future_1) =
            upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_1);

        let (_roothash, root_manifest_future) = upload_manifest_no_parents(
            ctx.clone(),
            &repo,
            format!("file\0{}\n", filehash_1),
            &RepoPath::root(),
        );

        let commit1 = create_changeset_no_parents(
            fb,
            &repo,
            root_manifest_future.map(Some).boxify(),
            vec![file_future_1],
        );

        let commit2 = {
            let fake_file_path_2 = RepoPath::file("FILE").expect("Can't generate fake RepoPath");
            let (filehash_2, file_future_2) =
                upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path_2);
            let (_roothash, root_manifest_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("FILE\0{}\n", filehash_2),
                &RepoPath::root(),
            );

            create_changeset_one_parent(
                fb,
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![file_future_2],
                commit1.clone(),
            )
        };

        assert!(run_future(
            commit1
                .get_completed_changeset()
                .join(commit2.get_completed_changeset()),
        )
        .is_ok());
    });
}

#[fbinit::test]
fn test_no_case_conflict_removal_dir(fb: FacebookInit) {
    async_unit::tokio_unit_test(move || {
        let ctx = CoreContext::test_mock(fb);
        let repo = get_empty_lazy_repo();

        let commit1 = {
            let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");
            let fake_dir_path = RepoPath::file("dir").expect("Can't generate fake RepoPath");
            let (filehash, file_future) =
                upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

            let (dirhash_1, manifest_dir_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("file\0{}\n", filehash),
                &fake_dir_path,
            );

            let (_roothash, root_manifest_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("dir\0{}t\n", dirhash_1),
                &RepoPath::root(),
            );

            create_changeset_no_parents(
                fb,
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![file_future, manifest_dir_future],
            )
        };

        let commit2 = {
            let fake_file_path = RepoPath::file("file").expect("Can't generate fake RepoPath");
            let fake_dir_path = RepoPath::file("DIR").expect("Can't generate fake RepoPath");
            let (filehash, file_future) =
                upload_file_no_parents(ctx.clone(), &repo, "blob", &fake_file_path);

            let (dirhash_1, manifest_dir_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("file\0{}\n", filehash),
                &fake_dir_path,
            );

            let (_roothash, root_manifest_future) = upload_manifest_no_parents(
                ctx.clone(),
                &repo,
                format!("DIR\0{}t\n", dirhash_1),
                &RepoPath::root(),
            );

            create_changeset_one_parent(
                fb,
                &repo,
                root_manifest_future.map(Some).boxify(),
                vec![file_future, manifest_dir_future],
                commit1.clone(),
            )
        };

        assert!(run_future(
            commit1
                .get_completed_changeset()
                .join(commit2.get_completed_changeset()),
        )
        .is_ok());
    });
}

#[fbinit::test]
fn test_hg_commit_generation_simple(fb: FacebookInit) {
    let repo = fixtures::linear::getrepo(fb);
    let bcs = create_bonsai_changeset(vec![]);

    let bcs_id = bcs.get_changeset_id();
    let ctx = CoreContext::test_mock(fb);

    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(blobrepo::save_bonsai_changesets(
            vec![bcs],
            ctx.clone(),
            repo.clone(),
        ))
        .unwrap();
    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx, bcs_id))
        .unwrap();
    assert_eq!(count, 1);
}

#[fbinit::test]
fn test_hg_commit_generation_stack(fb: FacebookInit) {
    let repo = fixtures::linear::getrepo(fb);
    let mut changesets = vec![];
    let bcs = create_bonsai_changeset(vec![]);

    let mut prev_bcs_id = bcs.get_changeset_id();
    changesets.push(bcs.clone());

    // Create a large stack to make sure we don't have stackoverflow problems
    let stack_size = 10000;
    for _ in 1..stack_size {
        let new_bcs = create_bonsai_changeset(vec![prev_bcs_id]);
        prev_bcs_id = new_bcs.get_changeset_id();
        changesets.push(new_bcs);
    }

    let top_of_stack = changesets.last().unwrap().clone().get_changeset_id();
    let ctx = CoreContext::test_mock(fb);
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    runtime
        .block_on(blobrepo::save_bonsai_changesets(
            changesets,
            ctx.clone(),
            repo.clone(),
        ))
        .unwrap();
    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx, top_of_stack))
        .unwrap();
    assert_eq!(count, stack_size);
}

#[fbinit::test]
fn test_hg_commit_generation_one_after_another(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    let repo = fixtures::linear::getrepo(fb);

    let first_bcs = create_bonsai_changeset(vec![]);
    let first_bcs_id = first_bcs.get_changeset_id();

    let second_bcs = create_bonsai_changeset(vec![first_bcs_id]);
    let second_bcs_id = second_bcs.get_changeset_id();

    runtime
        .block_on(blobrepo::save_bonsai_changesets(
            vec![first_bcs, second_bcs],
            ctx.clone(),
            repo.clone(),
        ))
        .unwrap();

    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx.clone(), first_bcs_id))
        .unwrap();
    assert_eq!(count, 1);

    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx, second_bcs_id))
        .unwrap();
    assert_eq!(count, 1);
}

#[fbinit::test]
fn test_hg_commit_generation_diamond(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    let repo = fixtures::linear::getrepo(fb);

    let last_bcs_id = runtime
        .block_on(fixtures::save_diamond_commits(
            ctx.clone(),
            repo.clone(),
            vec![],
        ))
        .unwrap();

    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx.clone(), last_bcs_id))
        .unwrap();
    assert_eq!(count, 4);
}

#[fbinit::test]
fn test_hg_commit_generation_many_diamond(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let mut runtime = tokio::runtime::Runtime::new().unwrap();
    let repo = fixtures::many_diamonds::getrepo(fb, &mut runtime);
    let book = bookmarks::BookmarkName::new("master").unwrap();
    let bcs_id = runtime
        .block_on(repo.get_bonsai_bookmark(ctx.clone(), &book))
        .unwrap()
        .unwrap();

    let (_, count) = runtime
        .block_on(repo.get_hg_from_bonsai_changeset_with_impl(ctx.clone(), bcs_id))
        .unwrap();
    assert_eq!(count, 200);
}

#[fbinit::test]
fn test_hg_commit_generation_uneven_branch(fb: FacebookInit) {
    let ctx = CoreContext::test_mock(fb);
    let repo = blobrepo_factory::new_memblob_empty(None).expect("cannot create empty repo");
    let mut runtime = tokio::runtime::Runtime::new().unwrap();

    let root_bcs = fixtures::create_bonsai_changeset(vec![]);

    let large_branch_1 = fixtures::create_bonsai_changeset(vec![root_bcs.get_changeset_id()]);
    let large_branch_2 = fixtures::create_bonsai_changeset(vec![large_branch_1.get_changeset_id()]);

    let short_branch = fixtures::create_bonsai_changeset(vec![root_bcs.get_changeset_id()]);

    let merge = fixtures::create_bonsai_changeset(vec![
        short_branch.get_changeset_id(),
        large_branch_2.get_changeset_id(),
    ]);

    let f = blobrepo::save_bonsai_changesets(
        vec![
            root_bcs,
            large_branch_1,
            large_branch_2,
            short_branch,
            merge.clone(),
        ],
        ctx.clone(),
        repo.clone(),
    );

    runtime.block_on(f).unwrap();
    runtime
        .block_on(repo.get_hg_from_bonsai_changeset(ctx.clone(), merge.get_changeset_id()))
        .unwrap();
}

#[fbinit::test]
fn save_reproducibility_under_load(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let delay_settings = DelaySettings {
        blobstore_put_dist: Normal::new(0.01, 0.005),
        blobstore_get_dist: Normal::new(0.005, 0.0025),
        db_put_dist: Normal::new(0.002, 0.001),
        db_get_dist: Normal::new(0.002, 0.001),
    };
    cmdlib::args::init_cachelib_from_settings(fb, Default::default())?;
    let repo = new_benchmark_repo(fb, delay_settings)?;

    let mut rng = XorShiftRng::seed_from_u64(1);
    let mut gen = GenManifest::new();
    let settings = Default::default();

    let test = gen
        .gen_stack(
            ctx.clone(),
            repo.clone(),
            &mut rng,
            &settings,
            None,
            std::iter::repeat(16).take(50),
        )
        .and_then(move |csid| repo.get_hg_from_bonsai_changeset(ctx, csid));

    let mut runtime = Runtime::new()?;
    assert_eq!(
        runtime.block_on(test)?,
        "6f67e722196896e645eec15b1d40fb0ecc5488d6".parse()?,
    );

    Ok(())
}

#[fbinit::test]
fn test_filenode_lookup(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let memblob = LazyMemblob::new();
    let blobstore = Arc::new(TracingBlobstore::new(memblob));

    let repo = blobrepo_factory::new_memblob_empty(Some(blobstore.clone()))?;

    let p1 = None;
    let p2 = None;

    let content_blob = FileContents::new_bytes(
        File::new(b"myblob".to_vec(), p1, p2)
            .file_contents()
            .into_bytes(),
    )
    .into_blob();
    let content_id = *content_blob.id();
    let content_len = content_blob.len() as u64;

    let mut rt = Runtime::new()?;
    let _ = rt.block_on(repo.upload_blob(ctx.clone(), content_blob))?;

    let path1 = RepoPath::file("path/1")?;
    let path2 = RepoPath::file("path/2")?;
    let path3 = RepoPath::file("path/3")?;

    let content_key = format!("repo0000.content.blake2.{}", content_id.to_hex());

    let cbmeta = ContentBlobMeta {
        id: content_id,
        size: content_len,
        copy_from: None,
    };

    let cbmeta_copy = ContentBlobMeta {
        id: content_id,
        size: content_len,
        copy_from: Some((to_mpath(path3.clone())?, ONES_FNID)),
    };

    // Clear our blobstore first.
    let _ = blobstore.tracing_gets();

    // First, upload. We expect 3 calls here:
    // - Filenode lookup: this will miss.
    // - File lookup (to compute metadata): this will hit.
    // - File lookup (to hash the contents): this will hit.

    let upload = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: UploadHgFileContents::ContentUploaded(cbmeta.clone()),
        file_type: FileType::Regular,
        p1,
        p2,
        path: to_mpath(path1.clone())?,
    };
    let (_, future) = upload.upload(ctx.clone(), repo.get_blobstore().boxed())?;

    let _ = rt.block_on(future)?;

    let gets = blobstore.tracing_gets();
    assert_eq!(gets.len(), 3);
    assert!(gets[0].contains("filenode_lookup"));
    assert_eq!(gets[1], content_key);
    assert_eq!(gets[2], content_key);

    // Now, upload the content again. This time, we expect one call to the alias, and one call to
    // fetch the metadata (this is obviously a little inefficient if we need both, but the latter
    // call can now be reduced to peeking at the file contents).

    let upload = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: UploadHgFileContents::ContentUploaded(cbmeta.clone()),
        file_type: FileType::Regular,
        p1,
        p2,
        path: to_mpath(path2.clone())?,
    };
    let (_, future) = upload.upload(ctx.clone(), repo.get_blobstore().boxed())?;
    let _ = rt.block_on(future)?;

    let gets = blobstore.tracing_gets();
    assert_eq!(gets.len(), 2);
    assert!(gets[0].contains("filenode_lookup"));
    assert_eq!(gets[1], content_key);

    // Finally, upload with different copy metadata. Reusing the filenode should not be possible,
    // so this should make 3 calls again.
    let upload = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Generate,
        contents: UploadHgFileContents::ContentUploaded(cbmeta_copy.clone()),
        file_type: FileType::Regular,
        p1,
        p2,
        path: to_mpath(path2.clone())?,
    };
    let (_, future) = upload.upload(ctx.clone(), repo.get_blobstore().boxed())?;
    let _ = rt.block_on(future)?;

    let gets = blobstore.tracing_gets();
    assert_eq!(gets.len(), 3);
    assert!(gets[0].contains("filenode_lookup"));
    assert_eq!(gets[1], content_key);
    assert_eq!(gets[2], content_key);

    Ok(())
}

#[fbinit::test]
fn test_content_uploaded_filenode_id(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);

    let repo = blobrepo_factory::new_memblob_empty(None)?;

    let p1 = None;
    let p2 = None;

    let content_blob = FileContents::new_bytes(
        File::new(b"myblob".to_vec(), p1, p2)
            .file_contents()
            .into_bytes(),
    )
    .into_blob();
    let content_id = *content_blob.id();
    let content_len = content_blob.len() as u64;

    let mut rt = Runtime::new()?;
    let _ = rt.block_on(repo.upload_blob(ctx.clone(), content_blob))?;

    let path1 = RepoPath::file("path/1")?;
    let path2 = RepoPath::file("path/2")?;

    let cbmeta = ContentBlobMeta {
        id: content_id,
        size: content_len,
        copy_from: Some((to_mpath(path2.clone())?, ONES_FNID)),
    };

    let upload = UploadHgFileEntry {
        upload_node_id: UploadHgNodeHash::Checked(
            "47f917b28e191c4bb0de8927e716e1b976ec3ad0".parse()?,
        ),
        contents: UploadHgFileContents::ContentUploaded(cbmeta.clone()),
        file_type: FileType::Regular,
        p1,
        p2,
        path: to_mpath(path1.clone())?,
    };
    let (_, future) = upload.upload(ctx.clone(), repo.get_blobstore().boxed())?;

    let _ = rt.block_on(future)?;

    Ok(())
}
