/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use super::HgFileNodeId;
use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use failure_ext::Error;
use futures::Future;
use std::sync::Arc;

mod envelope;
pub use envelope::HgBlobEnvelope;

mod errors;
pub use errors::ErrorKind;

pub mod file;
pub use file::{
    fetch_file_content_from_blobstore, fetch_file_content_id_from_blobstore,
    fetch_file_content_sha256_from_blobstore, fetch_file_contents, fetch_file_envelope,
    fetch_file_metadata_from_blobstore, fetch_file_parents_from_blobstore,
    fetch_file_size_from_blobstore, File, HgBlobEntry, LFSContent, META_MARKER, META_SZ,
};

mod manifest;
pub use self::manifest::{fetch_manifest_envelope, BlobManifest, ManifestContent};

mod changeset;
pub use changeset::{
    serialize_cs, serialize_extras, ChangesetMetadata, Extra, HgBlobChangeset, HgChangesetContent,
    RevlogChangeset,
};

pub mod filenode_lookup;

mod upload;
pub use upload::{
    ContentBlobInfo, ContentBlobMeta, UploadHgFileContents, UploadHgFileEntry, UploadHgNodeHash,
    UploadHgTreeEntry,
};

/// File metadata content in the same format as Mercurial stores in filelogs
pub fn fetch_raw_revlog_metadata(
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    node: HgFileNodeId,
) -> impl Future<Item = Bytes, Error = Error> {
    fetch_file_envelope(ctx.clone(), &blobstore, node).map(|envelope| envelope.metadata().clone())
}
