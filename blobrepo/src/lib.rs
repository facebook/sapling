// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

#[macro_use]
extern crate cloned;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate tokio;

extern crate bincode;
extern crate bonsai_utils;
extern crate bytes;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
#[macro_use]
extern crate slog;
#[macro_use]
extern crate stats;
extern crate tracing;
extern crate uuid;

extern crate heapsize;

extern crate futures_stats;

extern crate ascii;
extern crate blobstore;
extern crate blobstore_sync_queue;
extern crate bonsai_hg_mapping;
extern crate bookmarks;
extern crate cachelib;
extern crate changesets;
extern crate context;
extern crate crypto;
extern crate dbbookmarks;
extern crate delayblob;
extern crate fileblob;
extern crate filenodes;
#[macro_use]
extern crate futures_ext;
extern crate glusterblob;
extern crate manifoldblob;
#[cfg(test)]
#[macro_use]
extern crate maplit;
extern crate mercurial;
extern crate mercurial_types;
extern crate metaconfig;
extern crate mononoke_types;
#[cfg(test)]
extern crate mononoke_types_mocks;
extern crate multiplexedblob;
extern crate rocksblob;
extern crate rocksdb;
extern crate scribe;
extern crate scribe_cxx;
extern crate scuba;
extern crate scuba_ext;
extern crate sqlblob;
extern crate sqlfilenodes;
extern crate time_ext;

#[cfg(test)]
extern crate async_unit;
#[cfg(test)]
extern crate fixtures;
#[cfg(test)]
extern crate mercurial_types_mocks;

pub mod alias;
mod bonsai_generation;
mod changeset;
mod changeset_fetcher;
mod errors;
mod file;
mod manifest;
mod memory_manifest;
mod post_commit;
mod repo;
mod repo_commit;
mod utils;

pub use alias::*;
pub use changeset::{HgBlobChangeset, HgChangesetContent};
pub use changeset_fetcher::ChangesetFetcher;
pub use errors::*;
pub use file::HgBlobEntry;
pub use manifest::BlobManifest;
pub use repo::{
    save_bonsai_changesets, BlobRepo, ChangesetMetadata, ContentBlobInfo, ContentBlobMeta,
    CreateChangeset, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash, UploadHgTreeEntry,
};
pub use repo_commit::ChangesetHandle;
// TODO: This is exported for testing - is this the right place for it?
pub use repo_commit::compute_changed_files;

pub mod internal {
    pub use memory_manifest::{MemoryManifestEntry, MemoryRootManifest};
    pub use utils::{IncompleteFilenodeInfo, IncompleteFilenodes};
}

use failure::{err_msg, Error};
use futures::{future, Future, IntoFuture};
use futures_ext::FutureExt;
use metaconfig::RepoType;
use mononoke_types::RepositoryId;
use scribe_cxx::ScribeCxxClient;

pub fn open_blobrepo(
    logger: slog::Logger,
    repotype: RepoType,
    repoid: RepositoryId,
    myrouter_port: Option<u16>,
) -> impl Future<Item = BlobRepo, Error = Error> {
    use metaconfig::repoconfig::RepoType::*;

    match repotype {
        BlobFiles(ref path) => BlobRepo::new_files(logger, &path, repoid)
            .into_future()
            .left_future(),
        BlobRocks(ref path) => BlobRepo::new_rocksdb(logger, &path, repoid)
            .into_future()
            .left_future(),
        BlobSqlite(ref path) => BlobRepo::new_sqlite(logger, &path, repoid)
            .into_future()
            .left_future(),
        BlobRemote {
            ref blobstores_args,
            ref db_address,
            ref filenode_shards,
        } => {
            let myrouter_port = match myrouter_port {
                None => {
                    return future::err(err_msg(
                        "Missing myrouter port, unable to open BlobRemote repo",
                    ))
                    .left_future();
                }
                Some(myrouter_port) => myrouter_port,
            };
            BlobRepo::new_remote_scribe_commits(
                logger,
                blobstores_args,
                db_address.clone(),
                filenode_shards.clone(),
                repoid,
                myrouter_port,
                ScribeCxxClient::new(),
            )
            .right_future()
        }
    }
}
