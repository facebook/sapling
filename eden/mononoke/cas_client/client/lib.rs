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
use context::CoreContext;
pub use dummy::DummyCasClient;
pub use errors::ErrorKind;
#[cfg(fbcode_build)]
pub use facebook::casd_client::RemoteExecutionCasdClient;
use fbinit::FacebookInit;
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
    /// Get the name of the repo this client is for.
    fn repo_name(&self) -> &str;
}

pub fn build_mononoke_cas_client<'a>(
    fb: FacebookInit,
    ctx: &'a CoreContext,
    repo: &'a str,
    verbose: bool,
) -> Result<impl CasClient + 'a, Error> {
    #[cfg(fbcode_build)]
    {
        RemoteExecutionCasdClient::new(fb, ctx, repo, verbose)
    }
    #[cfg(not(fbcode_build))]
    {
        let _fb = fb; // unused
        let _verbose = verbose; // unused
        DummyCasClient::new(ctx, repo)
    }
}
