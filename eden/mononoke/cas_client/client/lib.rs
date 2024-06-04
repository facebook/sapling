/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dummy;
mod errors;
#[cfg(fbcode_build)]
mod facebook;

use anyhow::Error;
use bytes::Bytes;
pub use dummy::DummyCasClient;
pub use errors::ErrorKind;
#[cfg(fbcode_build)]
pub use facebook::casd_client::RemoteExecutionCasdClient;
use futures::Stream;
use mononoke_types::MononokeDigest;

#[async_trait::async_trait]
/// This trait provides an abstraction layer for pushing source control data to a CAS backend.
pub trait CasClient: Sync + Send {
    /// Upload given blob, having its digest and data stream.
    async fn streaming_upload_blob(
        &self,
        digest: &MononokeDigest,
        bytes_stream: impl Stream<Item = Result<Bytes, Error>> + Send,
    ) -> Result<(), Error>;
    /// Upload given blob, having its digest and data.
    async fn upload_blob(&self, digest: &MononokeDigest, bytes: Bytes) -> Result<(), Error>;
    /// Upload a list of blobs, having their digests and data.
    async fn upload_blobs(&self, blobs: Vec<(MononokeDigest, Bytes)>) -> Result<(), Error>;
    /// Lookup given digest in a Cas backend.
    async fn lookup_blob(&self, digest: &MononokeDigest) -> Result<bool, Error>;
    /// Lookup given digests in a Cas backend.
    async fn missing_digests<'a>(
        &self,
        digests: &'a [MononokeDigest],
    ) -> Result<Vec<MononokeDigest>, Error>;
}
