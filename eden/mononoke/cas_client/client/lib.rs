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
use slog::debug;

const CONFIGURED_PROD_REPOS: [&str; 2] = ["fbsource", "www"];

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

pub fn get_prod_usecase_from_reponame(ctx: &CoreContext, repo_name: &str) -> String {
    if CONFIGURED_PROD_REPOS.contains(&repo_name) {
        format!("source-control-{}", repo_name).to_string()
    } else {
        debug!(
            ctx.logger(),
            "Prod 'use case' hasn't been set up for the repo '{}', using test use case", repo_name
        );
        "source-control-testing".to_string()
    }
}

pub fn build_mononoke_cas_client<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    repo: &str,
    verbose: bool,
    use_case: &str,
) -> Result<impl CasClient + 'a, Error> {
    #[cfg(fbcode_build)]
    {
        RemoteExecutionCasdClient::new(fb, ctx, repo.to_owned(), verbose, use_case.to_owned())
    }
    #[cfg(not(fbcode_build))]
    {
        let _fb = fb; // unused
        let _verbose = verbose; // unused
        DummyCasClient::new(ctx, repo)
    }
}
