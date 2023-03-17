/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use bytes::Bytes;
use futures::future;
use futures::future::Future;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use mononoke_types::hash;

use crate::expected_size::ExpectedSize;
use crate::incremental_hash::GitSha1IncrementalHasher;
use crate::incremental_hash::Sha1IncrementalHasher;
use crate::incremental_hash::Sha256IncrementalHasher;
use crate::multiplexer::Multiplexer;
use crate::streamhash::hash_stream;

type Aliases = (hash::Sha1, hash::Sha256, hash::RichGitSha1);

/// Hashes are dependent on the ExpectedSize we received, so we want to make sure callers verify
/// that the size they gave us is actually the one they observed before giving them access to the
/// resulting hashes. To do so, we require callers to provide the effective size to retrieve
/// aliases.
#[derive(Debug)]
pub struct RedeemableAliases {
    expected_size: ExpectedSize,
    aliases: Aliases,
}

impl RedeemableAliases {
    pub fn new(expected_size: ExpectedSize, aliases: Aliases) -> Self {
        Self {
            expected_size,
            aliases,
        }
    }

    pub fn redeem(self, size: u64) -> Result<Aliases> {
        let Self {
            expected_size,
            aliases,
        } = self;
        expected_size.check_equals(size).map(move |_| aliases)
    }
}

// Given a multiplexer, attach new aliases computations to it.
pub fn add_aliases_to_multiplexer<T: AsRef<[u8]> + Send + Sync + Clone + 'static>(
    multiplexer: &mut Multiplexer<T>,
    expected_size: ExpectedSize,
) -> impl Future<Output = Result<RedeemableAliases, Error>> + std::marker::Unpin {
    let sha1 = multiplexer.add(|stream| hash_stream(Sha1IncrementalHasher::new(), stream));
    let sha256 = multiplexer.add(|stream| hash_stream(Sha256IncrementalHasher::new(), stream));
    let git_sha1 = multiplexer
        .add(move |stream| hash_stream(GitSha1IncrementalHasher::new(expected_size), stream));

    future::try_join3(sha1, sha256, git_sha1)
        .map_ok(move |aliases| RedeemableAliases::new(expected_size, aliases))
        .map_err(Error::from)
}

/// Produce hashes for a stream.
pub async fn alias_stream<S>(
    expected_size: ExpectedSize,
    chunks: S,
) -> Result<RedeemableAliases, Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Send,
{
    let mut multiplexer = Multiplexer::new();
    let aliases = add_aliases_to_multiplexer(&mut multiplexer, expected_size);

    multiplexer
        .drain(chunks)
        .await
        .map_err(|e| -> Error { e.into() })?;

    aliases.await
}
