/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Result;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use git2::Repository;
use mononoke_types::fsnode::FsnodeEntry;
use mononoke_types::hash;
use mononoke_types::ChangesetId;
use mononoke_types::FileType;
use mononoke_types::MPath;
use mononoke_types::MPathElement;
use mononoke_types::RepoPath;
use sorted_vector_map::SortedVectorMap;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::sync::mpsc;

mod git_walker;

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
    entries: SortedVectorMap<MPathElement, FsnodeEntry>,
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
                FsnodeEntry::File(_) => {
                    let entry_path =
                        RepoPath::dir(MPath::join_opt_element(path.mpath(), &filename))?;
                    bail!("{} is a file in Mononoke", entry_path);
                }
                FsnodeEntry::Directory(_) => {}
            },
            CheckEntry::File(filetype, sha256) => match fsnode_entry {
                FsnodeEntry::File(fsnode_file) => {
                    let entry_path =
                        RepoPath::file(MPath::join_opt_element(path.mpath(), &filename))?;
                    if *fsnode_file.file_type() != filetype {
                        bail!(
                            "{} is type {} in git and {} in Mononoke",
                            path,
                            filetype,
                            *fsnode_file.file_type()
                        );
                    }
                    if *fsnode_file.content_sha256() != sha256 {
                        bail!(
                            "{} has hash {} in git and {} in Mononoke",
                            entry_path,
                            sha256,
                            *fsnode_file.content_sha256()
                        );
                    }
                }
                FsnodeEntry::Directory(_) => {
                    let entry_path =
                        RepoPath::file(MPath::join_opt_element(path.mpath(), &filename))?;
                    bail!("{} is a directory in Mononoke", entry_path);
                }
            },
        }
    }

    // By this point, all the Mononoke entries have been checked
    for (filename, _) in contents.drain() {
        let entry_path = RepoPath::file(MPath::join_opt_element(path.mpath(), &filename))?;
        bail!("{} in git but not Bonsai", entry_path);
    }
    Ok(())
}

async fn check_receiver(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    cs: ChangesetId,
    rx: mpsc::Receiver<CheckNode>,
    scheduled_max: usize,
) -> Result<()> {
    let root_fsnode = RootFsnodeId::derive(ctx, blobrepo, cs)
        .await?
        .into_fsnode_id()
        .load(ctx, blobrepo.blobstore())
        .await?;

    let path_to_fsnode = Mutex::new(HashMap::new());
    let path_to_fsnode = &path_to_fsnode;
    path_to_fsnode
        .lock()
        .expect("lock poisoned")
        .insert(None, Some(root_fsnode));
    let rx = tokio_stream::wrappers::ReceiverStream::new(rx);

    rx.map(Result::Ok)
        .and_then(move |node| async move {
            // This relies on git_walker doing a top-down traversal and only visiting each directory once
            let path = &node.path;
            let this_fsnode = path_to_fsnode
                .lock()
                .expect("lock poisoned")
                .remove(&path.mpath().cloned())
                .ok_or_else(|| anyhow!("{} not found in Mononoke", path))?
                .ok_or_else(|| anyhow!("{} in git is a file in Mononoke", path))?;
            // Fetch all children in parallel, storing them in the map
            stream::iter(this_fsnode.list().map(Result::Ok))
                .try_for_each_concurrent(scheduled_max, move |(element, entry)| async move {
                    let old_entry = {
                        let fsnode = match entry {
                            FsnodeEntry::File(_) => None,
                            FsnodeEntry::Directory(dir) => {
                                Some(dir.id().load(ctx, blobrepo.blobstore()).await?)
                            }
                        };
                        path_to_fsnode
                            .lock()
                            .expect("lock poisoned")
                            .insert(Some(MPath::join_opt_element(path.mpath(), element)), fsnode)
                    };
                    if old_entry.is_some() {
                        Err(anyhow!("Two different routes to the same path?!?"))
                    } else {
                        Ok(())
                    }
                })
                .await?;

            // Pass on this node for comparison to git
            Ok((node, this_fsnode.into_subentries()))
        })
        .map_ok(|(node, fsnode)| check_node(node, fsnode))
        .try_for_each(|f| async move { tokio::spawn(f).await? })
        .await
}

pub async fn check_git_wc(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    cs: ChangesetId,
    git_repo: Repository,
    git_commit: String,
    git_lfs: bool,
    scheduled_max: usize,
) -> Result<()> {
    let (tx, rx) = mpsc::channel(100);
    let git_handle =
        tokio::task::spawn_blocking(move || git_walker::thread(git_repo, git_commit, git_lfs, tx));

    let res = check_receiver(ctx, blobrepo, cs, rx, scheduled_max).await;
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
