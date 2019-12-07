/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#[deny(warnings)]
use anyhow::{format_err, Error};
use derived_data::BonsaiDerived;
use futures::Future;
use futures::{
    future::IntoFuture,
    stream::{self, Stream},
};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt, StreamExt};
use git2::{ObjectType, Oid, Repository, Sort};
use std::collections::HashSet;
use std::collections::{BTreeMap, HashMap};
use std::convert::TryInto;
use std::path::Path;
use std::sync::{Arc, Mutex};

use blobrepo::save_bonsai_changesets;
use blobstore::Blobstore;
use clap::Arg;
use cmdlib::args;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::{self, FilestoreConfig, StoreRequest};
use git_types::{mode, TreeHandle, TreeMapping};
use manifest::{bonsai_diff, BonsaiDiffFileChange, Entry, Manifest, StoreLoadable};
use mononoke_types::{
    hash::GitSha1, BonsaiChangesetMut, ChangesetId, ContentMetadata, DateTime, FileChange,
    FileType, MPath, MPathElement,
};
use tokio::runtime::Runtime;

const ARG_GIT_REPOSITORY_PATH: &str = "git-repository-path";
const ARG_DERIVE_TREES: &str = "derive-trees";
const ARG_HGGIT_COMPATIBILITY: &str = "hggit-compatibility";

const HGGIT_COMMIT_ID_EXTRA: &str = "convert_revision";

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
struct GitTree(Oid);

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
struct GitLeaf(Oid);

struct GitManifest(HashMap<MPathElement, Entry<GitTree, (FileType, GitLeaf)>>);

impl Manifest for GitManifest {
    type TreeId = GitTree;
    type LeafId = (FileType, GitLeaf);

    fn lookup(&self, name: &MPathElement) -> Option<Entry<Self::TreeId, Self::LeafId>> {
        self.0.get(name).cloned()
    }

    fn list(&self) -> Box<dyn Iterator<Item = (MPathElement, Entry<Self::TreeId, Self::LeafId>)>> {
        Box::new(self.0.clone().into_iter())
    }
}

fn load_git_tree(oid: Oid, repo: &Repository) -> Result<GitManifest, Error> {
    let tree = repo.find_tree(oid)?;

    let elements: Result<HashMap<_, _>, Error> = tree
        .iter()
        .map(|entry| {
            let oid = entry.id();
            let filemode = entry.filemode();
            let name = MPathElement::new(entry.name_bytes().into())?;

            let r = match entry.kind() {
                Some(ObjectType::Blob) => {
                    let ft = match filemode {
                        mode::GIT_FILEMODE_BLOB => FileType::Regular,
                        mode::GIT_FILEMODE_BLOB_EXECUTABLE => FileType::Executable,
                        mode::GIT_FILEMODE_LINK => FileType::Symlink,
                        _ => Err(format_err!("Invalid filemode: {:?}", filemode))?,
                    };

                    (name, Entry::Leaf((ft, GitLeaf(oid))))
                }
                Some(ObjectType::Tree) => (name, Entry::Tree(GitTree(oid))),
                k => Err(format_err!("Invalid kind: {:?}", k))?,
            };

            Ok(r)
        })
        .collect();

    Ok(GitManifest(elements?))
}

impl StoreLoadable<Arc<Mutex<Repository>>> for GitTree {
    type Value = GitManifest;

    fn load(
        &self,
        _ctx: CoreContext,
        store: &Arc<Mutex<Repository>>,
    ) -> BoxFuture<Self::Value, Error> {
        let repo = store.lock().expect("Poisoned lock");
        load_git_tree(self.0, &repo).into_future().boxify()
    }
}

fn do_upload(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    repo: Arc<Mutex<Repository>>,
    oid: Oid,
) -> BoxFuture<ContentMetadata, Error> {
    let repo = repo.lock().expect("Poisoned lock");
    let blob = try_boxfuture!(repo.find_blob(oid));
    let bytes = blob.content();
    let size = bytes.len().try_into().unwrap();

    let git_sha1 = try_boxfuture!(GitSha1::from_bytes(blob.id().as_bytes(), "blob", size));
    let req = StoreRequest::with_git_sha1(size, git_sha1);
    filestore::store(
        blobstore,
        &FilestoreConfig::default(),
        ctx,
        &req,
        stream::once(Ok(bytes.into())),
    )
    .boxify()
}

// TODO: Try to produce copy-info?
// TODO: Translate LFS pointers?
// TODO: Don't re-upload things we already have
fn find_file_changes<S>(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    repo: Arc<Mutex<Repository>>,
    changes: S,
) -> impl Future<Item = BTreeMap<MPath, Option<FileChange>>, Error = Error>
where
    S: Stream<Item = BonsaiDiffFileChange<GitLeaf>, Error = Error>,
{
    changes
        .map(move |change| match change {
            BonsaiDiffFileChange::Changed(path, ty, GitLeaf(oid))
            | BonsaiDiffFileChange::ChangedReusedId(path, ty, GitLeaf(oid)) => {
                do_upload(ctx.clone(), blobstore.clone(), repo.clone(), oid)
                    .map(move |meta| {
                        (
                            path,
                            Some(FileChange::new(meta.content_id, ty, meta.total_size, None)),
                        )
                    })
                    .left_future()
            }
            BonsaiDiffFileChange::Deleted(path) => Ok((path, None)).into_future().right_future(),
        })
        .buffer_unordered(100)
        .collect_to()
        .from_err()
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke Git Importer")
        .with_advanced_args_hidden()
        .build()
        .arg(
            Arg::with_name(ARG_DERIVE_TREES)
                .long(ARG_DERIVE_TREES)
                .required(false)
                .takes_value(false),
        )
        .arg(
            Arg::with_name(ARG_HGGIT_COMPATIBILITY)
                .long(ARG_HGGIT_COMPATIBILITY)
                .help("Set commit extras for hggit compatibility")
                .required(false)
                .takes_value(false),
        )
        .arg(Arg::with_name(ARG_GIT_REPOSITORY_PATH).help("Path to a git repository to import"));

    let matches = app.get_matches();
    let mut runtime = Runtime::new()?;

    args::init_cachelib(fb, &matches);
    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo = runtime.block_on(args::open_repo(fb, &logger, &matches))?;
    let tree_mapping = TreeMapping::new(repo.get_blobstore().boxed());

    let derive_trees = matches.is_present(ARG_DERIVE_TREES);
    let hggit_compatibility = matches.is_present(ARG_HGGIT_COMPATIBILITY);
    let path = Path::new(matches.value_of(ARG_GIT_REPOSITORY_PATH).unwrap());

    let walk_repo = Repository::open(&path)?;
    let store_repo = Arc::new(Mutex::new(Repository::open(&path)?));

    let mut walk = walk_repo.revwalk()?;
    walk.set_sorting(Sort::TOPOLOGICAL | Sort::REVERSE);

    for reference in walk_repo.references()? {
        let reference = reference?;
        if let Some(oid) = reference.target() {
            walk.push(oid)?;
        }
    }

    // TODO: Don't import everything in one go. Instead, hide things we already imported from the
    // traversal.

    let mut import_map: HashMap<Oid, ChangesetId> = HashMap::new();
    let mut changesets = Vec::new();

    for commit in walk {
        let commit = walk_repo.find_commit(commit?)?;
        let root = GitTree(commit.tree()?.id());

        let parents: Result<HashSet<_>, Error> = commit
            .parents()
            .map(|p| {
                let tree = p.tree()?;
                Ok(GitTree(tree.id()))
            })
            .collect();

        let diff = bonsai_diff(ctx.clone(), store_repo.clone(), root, parents?);

        // TODO: Include email in the author
        let author = commit
            .author()
            .name()
            .ok_or(format_err!("Commit has no author: {:?}", commit.id()))?
            .to_owned();
        let message = commit.message().unwrap_or_default().to_owned();

        // TODO: Use a Git <-> Bonsai mapping
        let parents = commit
            .parents()
            .map(|p| {
                let e = format_err!("Commit was not imported: {}", p.id());
                import_map.get(&p.id()).cloned().ok_or(e)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let file_changes = runtime.block_on(find_file_changes(
            ctx.clone(),
            repo.get_blobstore().boxed(),
            store_repo.clone(),
            diff,
        ))?;

        let time = commit.time();

        let mut extra = BTreeMap::new();
        if hggit_compatibility {
            extra.insert(
                HGGIT_COMMIT_ID_EXTRA.to_string(),
                commit.id().to_string().into_bytes(),
            );
        }

        // TODO: Should we have furhter extras?
        let bonsai_cs = BonsaiChangesetMut {
            parents: parents.clone(),
            author,
            author_date: DateTime::from_timestamp(time.seconds(), time.offset_minutes() * 60)?,
            committer: None,
            committer_date: None,
            message,
            extra,
            file_changes,
        }
        .freeze()?;

        let bcs_id = bonsai_cs.get_changeset_id();
        changesets.push(bonsai_cs);

        import_map.insert(commit.id(), bcs_id);

        println!("Created {:?} => {:?}", commit.id(), bcs_id);
    }

    runtime.block_on(save_bonsai_changesets(
        changesets,
        ctx.clone(),
        repo.clone(),
    ))?;

    for reference in walk_repo.references()? {
        let reference = reference?;

        let commit = reference.peel_to_commit()?;
        let bcs_id = import_map.get(&commit.id());
        println!("Ref: {:?}: {:?}", reference.name(), bcs_id);
    }

    if derive_trees {
        for (id, bcs_id) in import_map.iter() {
            let commit = walk_repo.find_commit(*id)?;
            let tree_id = commit.tree()?.id();

            let derived_tree = runtime.block_on(TreeHandle::derive(
                ctx.clone(),
                repo.clone(),
                tree_mapping.clone(),
                *bcs_id,
            ))?;

            let derived_tree_id = Oid::from_bytes(derived_tree.oid().as_ref())?;

            if tree_id != derived_tree_id {
                let e = format_err!(
                    "Invalid tree was derived for {:?}: {:?} (expected {:?})",
                    commit.id(),
                    derived_tree_id,
                    tree_id
                );
                Err(e)?;
            }
        }

        println!("{} tree(s) are valid!", import_map.len());
    }

    Ok(())
}
