/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Error;
use blobstore::Blobstore;
use context::CoreContext;
use fbthrift::compact_protocol;
use filenodes::FilenodeInfo;
use filenodes::FilenodesRef;
use filenodes::PreparedFilenode;
use futures::future;
use futures::stream::Stream;
use futures::stream::StreamExt;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mononoke_types::BlobstoreBytes;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use slog::info;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;

mod thrift {
    pub use microwave_if::*;
}

#[derive(Debug, Copy, Clone)]
pub enum SnapshotLocation<'a> {
    SharedLocalPath(&'a Path),
    Blobstore,
}

pub struct Snapshot {
    snapshot: thrift::RepoSnapshot,
}

impl Snapshot {
    pub async fn build<FilenodesStream>(filenodes: FilenodesStream) -> Self
    where
        FilenodesStream: Stream<Item = PreparedFilenode>,
    {
        let mut seen_filenodes = HashSet::new();
        let filenodes = filenodes
            .fold(Vec::new(), |mut v, c| {
                let PreparedFilenode { path, info } = c;

                if seen_filenodes.insert((path.clone(), info.filenode.clone())) {
                    let t = thrift::FilenodeSnapshot {
                        path: Some(path.into_thrift()),
                        filenode: Some(info.filenode.into_nodehash().into_thrift()),
                        p1: info.p1.map(|p| p.into_nodehash().into_thrift()),
                        p2: info.p2.map(|p| p.into_nodehash().into_thrift()),
                        copyfrom: info.copyfrom.map(|copyfrom| thrift::CopyInfoSnapshot {
                            path: Some(copyfrom.0.into_thrift()),
                            filenode: Some(copyfrom.1.into_nodehash().into_thrift()),
                        }),
                        linknode: Some(info.linknode.into_nodehash().into_thrift()),
                    };

                    v.push(t);
                }

                future::ready(v)
            })
            .await;

        Snapshot {
            snapshot: thrift::RepoSnapshot {
                filenodes: Some(filenodes),
                changesets: Some(vec![]),
            },
        }
    }

    pub async fn commit(
        &self,
        ctx: &CoreContext,
        repo: &(impl RepoIdentityRef + RepoBlobstoreRef),
        location: SnapshotLocation<'_>,
    ) -> Result<(), Error> {
        let serialized = compact_protocol::serialize(&self.snapshot);

        match location {
            SnapshotLocation::SharedLocalPath(path) => {
                let mut file = File::create(snapshot_path(path, repo.repo_identity().id())).await?;
                file.write_all(&serialized).await?;
            }
            SnapshotLocation::Blobstore => {
                repo.repo_blobstore()
                    .put(ctx, snapshot_name(), BlobstoreBytes::from_bytes(serialized))
                    .await?;
            }
        };

        Ok(())
    }
}

fn snapshot_name() -> String {
    format!("microwave_snapshot_v{}", thrift::CODEVER)
}

fn snapshot_path(shared_local_path: &Path, repo_id: RepositoryId) -> PathBuf {
    let name = format!("{}{}", repo_id.prefix(), snapshot_name());
    shared_local_path.join(name)
}

async fn load_snapshot(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoBlobstoreRef),
    location: SnapshotLocation<'_>,
) -> Result<thrift::RepoSnapshot, Error> {
    match location {
        SnapshotLocation::SharedLocalPath(path) => {
            let mut contents = vec![];
            let mut snapshot = File::open(snapshot_path(path, repo.repo_identity().id())).await?;
            snapshot.read_to_end(&mut contents).await?;
            Ok(compact_protocol::deserialize(&contents)?)
        }
        SnapshotLocation::Blobstore => {
            let bytes = repo
                .repo_blobstore()
                .get(ctx, &snapshot_name())
                .await?
                .ok_or_else(|| Error::msg("Snapshot is missing"))?
                .into_raw_bytes();
            Ok(compact_protocol::deserialize(&bytes)?)
        }
    }
}

pub async fn prime_cache(
    ctx: &CoreContext,
    repo: &(impl RepoIdentityRef + RepoBlobstoreRef + FilenodesRef),
    location: SnapshotLocation<'_>,
) -> Result<(), Error> {
    let snapshot = load_snapshot(ctx, repo, location).await?;

    let filenodes = snapshot
        .filenodes
        .ok_or_else(|| Error::msg("filenodes missing"))?;
    let filenodes = reheat_filenodes(filenodes)?;

    repo.filenodes().prime_cache(ctx, filenodes.as_ref());
    info!(
        ctx.logger(),
        "primed filenodes cache with {} entries",
        filenodes.len()
    );

    Ok(())
}

fn reheat_filenodes(
    filenodes: Vec<thrift::FilenodeSnapshot>,
) -> Result<Vec<PreparedFilenode>, Error> {
    filenodes
        .into_iter()
        .map(|t| {
            let thrift::FilenodeSnapshot {
                path,
                filenode,
                p1,
                p2,
                copyfrom,
                linknode,
            } = t;

            let path = path.ok_or_else(|| Error::msg("path missing"))?;
            let filenode = filenode.ok_or_else(|| Error::msg("filenode missing"))?;
            let linknode = linknode.ok_or_else(|| Error::msg("linknode missing"))?;

            let copyfrom = copyfrom
                .map(|t| {
                    let thrift::CopyInfoSnapshot { path, filenode } = t;
                    let path = path.ok_or_else(|| Error::msg("copy info path missing"))?;
                    let filenode =
                        filenode.ok_or_else(|| Error::msg("copy info filenode missing"))?;
                    Result::<_, Error>::Ok((
                        RepoPath::from_thrift(path)?,
                        HgFileNodeId::new(HgNodeHash::from_thrift(filenode)?),
                    ))
                })
                .transpose()?;

            let filenode = HgFileNodeId::new(HgNodeHash::from_thrift(filenode)?);

            Ok(PreparedFilenode {
                path: RepoPath::from_thrift(path)?,
                info: FilenodeInfo {
                    filenode,
                    p1: HgNodeHash::from_thrift_opt(p1)?.map(HgFileNodeId::new),
                    p2: HgNodeHash::from_thrift_opt(p2)?.map(HgFileNodeId::new),
                    copyfrom,
                    linknode: HgChangesetId::new(HgNodeHash::from_thrift(linknode)?),
                },
            })
        })
        .collect()
}
