/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use blobstore::Loadable;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use either::Either;
use fsnodes::RootFsnodeId;
use futures::StreamExt;
use futures::TryStreamExt;
use git2::Repository;
use manifest::Entry;
use manifest::Manifest;
use mononoke_types::hash;
use mononoke_types::hash::Sha256;
use mononoke_types::path::MPath;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use sorted_vector_map::SortedVectorMap;
use tokio::sync::mpsc;

mod git_walker;

pub trait HgRepo = RepoBlobstoreRef + RepoDerivedDataRef + RepoIdentityRef + Send + Sync;

#[derive(Debug)]
pub(crate) enum CheckEntry {
    Directory,
    File(FileType, hash::Sha256),
}

#[derive(Debug)]
pub(crate) struct CheckNode {
    pub path: RepoPath,
    pub contents: HashMap<MPathElement, CheckEntry>,
}

async fn check_node(
    node: CheckNode,
    entries: SortedVectorMap<MPathElement, Entry<(), (FileType, Sha256)>>,
) -> Result<()> {
    let CheckNode { path, mut contents } = node;

    // Check that each Mononoke entry is the same as its matching git entry
    // and remove it from contents once checked
    for (filename, fsnode_entry) in entries {
        let git_entry = contents
            .remove(&filename)
            .ok_or_else(|| anyhow!("File {}/{} in Bonsai but not git", path, filename))?;
        match git_entry {
            CheckEntry::Directory => match fsnode_entry {
                Entry::Leaf(_) => {
                    let entry_path =
                        RepoPath::dir(NonRootMPath::join_opt_element(path.mpath(), &filename))?;
                    bail!("{} is a file in Mononoke", entry_path);
                }
                Entry::Tree(_) => {}
            },
            CheckEntry::File(git_file_type, git_sha256) => match fsnode_entry {
                Entry::Leaf((file_type, sha256)) => {
                    let entry_path =
                        RepoPath::file(NonRootMPath::join_opt_element(path.mpath(), &filename))?;
                    if file_type != git_file_type {
                        bail!(
                            "{} is type {} in git and {} in Mononoke",
                            path,
                            git_file_type,
                            file_type,
                        );
                    }
                    if sha256 != git_sha256 {
                        bail!(
                            "{} has hash {} in git and {} in Mononoke",
                            entry_path,
                            git_sha256,
                            sha256,
                        );
                    }
                }
                Entry::Tree(_) => {
                    let entry_path =
                        RepoPath::file(NonRootMPath::join_opt_element(path.mpath(), &filename))?;
                    bail!("{} is a directory in Mononoke", entry_path);
                }
            },
        }
    }

    // By this point, all the Mononoke entries have been checked
    if let Some((filename, _)) = contents.drain().next() {
        let entry_path = RepoPath::file(NonRootMPath::join_opt_element(path.mpath(), &filename))?;
        bail!("{} in git but not Bonsai", entry_path);
    }

    Ok(())
}

async fn check_receiver(
    ctx: &CoreContext,
    hg_repo: &impl HgRepo,
    cs: ChangesetId,
    rx: mpsc::Receiver<CheckNode>,
    scheduled_max: usize,
) -> Result<()> {
    let root_content_mf = if let Ok(true) = justknobs::eval(
        "scm/mononoke:derived_data_use_content_manifests",
        None,
        Some(hg_repo.repo_identity().name()),
    ) {
        Either::Left(
            hg_repo
                .repo_derived_data()
                .derive::<RootContentManifestId>(ctx, cs)
                .await?
                .into_content_manifest_id()
                .load(ctx, hg_repo.repo_blobstore())
                .await?,
        )
    } else {
        Either::Right(
            hg_repo
                .repo_derived_data()
                .derive::<RootFsnodeId>(ctx, cs)
                .await?
                .into_fsnode_id()
                .load(ctx, hg_repo.repo_blobstore())
                .await?,
        )
    };

    let path_to_content_mf = Mutex::new(HashMap::new());
    let path_to_content_mf = &path_to_content_mf;
    path_to_content_mf
        .lock()
        .expect("lock poisoned")
        .insert(MPath::ROOT, Some(root_content_mf));
    let rx = tokio_stream::wrappers::ReceiverStream::new(rx);

    rx.map(Result::Ok)
        .and_then(move |node| async move {
            // This relies on git_walker doing a top-down traversal and only visiting each directory once
            let path = &node.path;
            let this_content_mf = path_to_content_mf
                .lock()
                .expect("lock poisoned")
                .remove(path.mpath().into())
                .ok_or_else(|| anyhow!("{} not found in Mononoke", path))?
                .ok_or_else(|| anyhow!("{} in git is a file in Mononoke", path))?;
            // Fetch all children in parallel, storing them in the map
            this_content_mf
                .list(ctx, hg_repo.repo_blobstore())
                .await?
                .try_for_each_concurrent(scheduled_max, move |(element, entry)| async move {
                    let old_entry = {
                        let content_mf = match entry {
                            Entry::Leaf(_) => None,
                            Entry::Tree(id) => Some(id.load(ctx, hg_repo.repo_blobstore()).await?),
                        };
                        path_to_content_mf.lock().expect("lock poisoned").insert(
                            <&MPath>::from(path.mpath()).join_element(Some(&element)),
                            content_mf,
                        )
                    };
                    if old_entry.is_some() {
                        Err(anyhow!("Two different routes to the same path?!?"))
                    } else {
                        Ok(())
                    }
                })
                .await?;

            // Collect the type and sha256 of the subentries and pass on to git for comparison
            let subentries = this_content_mf
                .list(ctx, hg_repo.repo_blobstore())
                .await?
                .map_ok(|(element, entry)| async move {
                    match entry {
                        Entry::Tree(_) => Ok((element, Entry::Tree(()))),
                        Entry::Leaf(Either::Left(file)) => {
                            let metadata = filestore::get_metadata(
                                hg_repo.repo_blobstore(),
                                ctx,
                                &file.content_id.into(),
                            )
                            .await?
                            .ok_or_else(|| {
                                anyhow!("Content metadata missing for {}", file.content_id)
                            })?;
                            Ok((element, Entry::Leaf((file.file_type, metadata.sha256))))
                        }
                        Entry::Leaf(Either::Right(file)) => Ok((
                            element,
                            Entry::Leaf((file.file_type().clone(), file.content_sha256().clone())),
                        )),
                    }
                })
                .try_buffered(100)
                .try_collect()
                .await?;

            Ok((node, subentries))
        })
        .map_ok(|(node, subentries)| check_node(node, subentries))
        .try_for_each(|f| async move { tokio::spawn(f).await? })
        .await
}

pub async fn check_git_wc(
    ctx: &CoreContext,
    hg_repo: &impl HgRepo,
    cs: ChangesetId,
    git_repo: Repository,
    git_commit: String,
    git_lfs: bool,
    scheduled_max: usize,
) -> Result<()> {
    let (tx, rx) = mpsc::channel(100);
    let git_handle =
        tokio::task::spawn_blocking(move || git_walker::thread(git_repo, git_commit, git_lfs, tx));

    let res = check_receiver(ctx, hg_repo, cs, rx, scheduled_max).await;
    let git_res = git_handle.await?;
    match (res, git_res) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(e), Ok(())) | (Ok(()), Err(e)) => Err(e),
        (Err(e1), Err(e2)) => Err(anyhow!(
            "Both git and Mononoke threads failed.\ngit: {}\nMononoke: {}\n",
            e2,
            e1
        )),
    }
}
