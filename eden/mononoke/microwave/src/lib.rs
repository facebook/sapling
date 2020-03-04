/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use context::CoreContext;
use fbthrift::compact_protocol;
use filenodes::{FilenodeInfo, PreparedFilenode};
use futures::{
    compat::Future01CompatExt,
    future,
    stream::{Stream, StreamExt},
};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgNodeHash};
use mononoke_types::{BlobstoreBytes, RepoPath, RepositoryId};
use std::path::{Path, PathBuf};
use tokio::{
    fs::File,
    io::{AsyncReadExt, AsyncWriteExt},
};

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
        let filenodes = filenodes
            .fold(Vec::new(), |mut v, c| {
                let PreparedFilenode { path, info } = c;

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

                future::ready(v)
            })
            .await;

        Snapshot {
            snapshot: thrift::RepoSnapshot {
                filenodes: Some(filenodes),
            },
        }
    }

    pub async fn commit(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        location: SnapshotLocation<'_>,
    ) -> Result<(), Error> {
        let serialized = compact_protocol::serialize(&self.snapshot);

        match location {
            SnapshotLocation::SharedLocalPath(ref path) => {
                let mut file = File::create(snapshot_path(path, repo.get_repoid())).await?;
                file.write_all(&serialized).await?;
            }
            SnapshotLocation::Blobstore => {
                repo.blobstore()
                    .put(
                        ctx.clone(),
                        snapshot_name(),
                        BlobstoreBytes::from_bytes(serialized),
                    )
                    .compat()
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
    shared_local_path.join(&name)
}

async fn load_snapshot(
    ctx: &CoreContext,
    repo: &BlobRepo,
    location: SnapshotLocation<'_>,
) -> Result<thrift::RepoSnapshot, Error> {
    match location {
        SnapshotLocation::SharedLocalPath(ref path) => {
            let mut contents = vec![];
            let mut snapshot = File::open(snapshot_path(path, repo.get_repoid())).await?;
            snapshot.read_to_end(&mut contents).await?;
            Ok(compact_protocol::deserialize(&contents)?)
        }
        SnapshotLocation::Blobstore => {
            let bytes = repo
                .get_blobstore()
                .get(ctx.clone(), snapshot_name())
                .compat()
                .await?
                .ok_or(Error::msg("Snapshot is missing"))?
                .into_bytes();
            Ok(compact_protocol::deserialize(&bytes)?)
        }
    }
}

pub async fn prime_cache(
    ctx: &CoreContext,
    repo: &BlobRepo,
    location: SnapshotLocation<'_>,
) -> Result<(), Error> {
    let snapshot = load_snapshot(ctx, repo, location).await?;

    let filenodes = snapshot.filenodes.ok_or(Error::msg("filenodes missing"))?;
    let filenodes = reheat_filenodes(filenodes)?;

    repo.get_filenodes()
        .prime_cache(ctx, repo.get_repoid(), filenodes.as_ref());

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

            let path = path.ok_or(Error::msg("path missing"))?;
            let filenode = filenode.ok_or(Error::msg("filenode missing"))?;
            let linknode = linknode.ok_or(Error::msg("linknode missing"))?;

            let copyfrom = copyfrom
                .map(|t| {
                    let thrift::CopyInfoSnapshot { path, filenode } = t;
                    let path = path.ok_or(Error::msg("copy info path missing"))?;
                    let filenode = filenode.ok_or(Error::msg("copy info filenode missing"))?;
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
