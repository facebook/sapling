/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use futures::future;
use futures::future::Future;
use futures::future::TryFutureExt;
use mononoke_types::hash;

use crate::expected_size::ExpectedSize;
use crate::incremental_hash::Blake3IncrementalHasher;
use crate::incremental_hash::GitSha1IncrementalHasher;
use crate::incremental_hash::Sha1IncrementalHasher;
use crate::incremental_hash::Sha256IncrementalHasher;
use crate::multiplexer::Multiplexer;
use crate::streamhash::hash_stream;

type Aliases = (hash::Sha1, hash::Sha256, hash::RichGitSha1, hash::Blake3);

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
) -> impl Future<Output = Result<RedeemableAliases, Error>> + std::marker::Unpin + use<T> {
    let sha1 = multiplexer.add(|stream| hash_stream(Sha1IncrementalHasher::new(), stream));
    let sha256 = multiplexer.add(|stream| hash_stream(Sha256IncrementalHasher::new(), stream));
    let git_sha1 = multiplexer
        .add(move |stream| hash_stream(GitSha1IncrementalHasher::new(expected_size), stream));
    let seeded_blake3 =
        multiplexer.add(|stream| hash_stream(Blake3IncrementalHasher::new_seeded(), stream));

    future::try_join4(sha1, sha256, git_sha1, seeded_blake3)
        .map_ok(move |aliases| RedeemableAliases::new(expected_size, aliases))
        .map_err(Error::from)
}
