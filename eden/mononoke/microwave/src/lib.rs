/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use changesets::ChangesetEntry;
use changesets::ChangesetsRef;
use context::CoreContext;
use fbthrift::compact_protocol;
use filenodes::FilenodeInfo;
use filenodes::PreparedFilenode;
use futures::future;
use futures::stream::Stream;
use futures::stream::StreamExt;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgNodeHash;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use mononoke_types::RepoPath;
use mononoke_types::RepositoryId;
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
    pub async fn build<FilenodesStream, ChangesetsStream>(
        filenodes: FilenodesStream,
        changesets: ChangesetsStream,
    ) -> Self
    where
        FilenodesStream: Stream<Item = PreparedFilenode>,
        ChangesetsStream: Stream<Item = ChangesetEntry>,
    {
        let filenodes = filenodes.fold(Vec::new(), |mut v, c| {
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
        });

        let changesets = changesets.fold(Vec::new(), |mut v, c| {
            let ChangesetEntry {
                repo_id: _,
                cs_id,
                parents,
                gen,
            } = c;

            let t = thrift::ChangesetSnapshot {
                cs_id: Some(cs_id.into_thrift()),
                parents: Some(parents.into_iter().map(|p| p.into_thrift()).collect()),
                // NOTE: We expect this conversion (and the reverse one) between u64 and i64 to
                // succeed because the generation number is >= 0, but also not so large that it
                // cannot fit in a i64.
                gen: Some(gen.try_into().unwrap()),
            };

            v.push(t);

            future::ready(v)
        });

        let (filenodes, changesets) = future::join(filenodes, changesets).await;

        Snapshot {
            snapshot: thrift::RepoSnapshot {
                filenodes: Some(filenodes),
                changesets: Some(changesets),
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
            SnapshotLocation::SharedLocalPath(path) => {
                let mut file = File::create(snapshot_path(path, repo.get_repoid())).await?;
                file.write_all(&serialized).await?;
            }
            SnapshotLocation::Blobstore => {
                repo.blobstore()
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
    shared_local_path.join(&name)
}

async fn load_snapshot(
    ctx: &CoreContext,
    repo: &BlobRepo,
    location: SnapshotLocation<'_>,
) -> Result<thrift::RepoSnapshot, Error> {
    match location {
        SnapshotLocation::SharedLocalPath(path) => {
            let mut contents = vec![];
            let mut snapshot = File::open(snapshot_path(path, repo.get_repoid())).await?;
            snapshot.read_to_end(&mut contents).await?;
            Ok(compact_protocol::deserialize(&contents)?)
        }
        SnapshotLocation::Blobstore => {
            let bytes = repo
                .blobstore()
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
    repo: &BlobRepo,
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

    let changesets = snapshot
        .changesets
        .ok_or_else(|| Error::msg("changesets missing"))?;
    let changesets = reheat_changesets(repo.get_repoid(), changesets)?;

    repo.changesets().prime_cache(ctx, changesets.as_ref());
    info!(
        ctx.logger(),
        "primed changesets cache with {} entries",
        changesets.len()
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

fn reheat_changesets(
    repo_id: RepositoryId,
    changesets: Vec<thrift::ChangesetSnapshot>,
) -> Result<Vec<ChangesetEntry>, Error> {
    changesets
        .into_iter()
        .map(|c| {
            let thrift::ChangesetSnapshot {
                cs_id,
                parents,
                gen,
            } = c;

            let cs_id = cs_id.ok_or_else(|| Error::msg("cs_id missing"))?;
            let parents = parents.ok_or_else(|| Error::msg("parents missing"))?;
            let gen = gen.ok_or_else(|| Error::msg("gen missing"))?;

            let parents = parents
                .into_iter()
                .map(ChangesetId::from_thrift)
                .collect::<Result<Vec<_>, _>>()?;

            Ok(ChangesetEntry {
                repo_id,
                cs_id: ChangesetId::from_thrift(cs_id)?,
                parents,
                gen: gen.try_into().unwrap(), // See above
            })
        })
        .collect()
}
