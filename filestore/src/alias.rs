// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use failure_ext::{Error, Result};
use futures::{
    future::{lazy, IntoFuture},
    Future, Stream,
};
use mononoke_types::hash;

use crate::expected_size::ExpectedSize;
use crate::incremental_hash::{
    GitSha1IncrementalHasher, Sha1IncrementalHasher, Sha256IncrementalHasher,
};
use crate::multiplexer::Multiplexer;
use crate::spawn::SpawnError;
use crate::streamhash::hash_stream;

type Aliases = (hash::Sha1, hash::Sha256, hash::GitSha1);

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
) -> impl Future<Item = RedeemableAliases, Error = SpawnError<!>> {
    let sha1 = multiplexer.add(|stream| hash_stream(Sha1IncrementalHasher::new(), stream));
    let sha256 = multiplexer.add(|stream| hash_stream(Sha256IncrementalHasher::new(), stream));
    let git_sha1 = multiplexer
        .add(move |stream| hash_stream(GitSha1IncrementalHasher::new(expected_size), stream));

    (sha1, sha256, git_sha1)
        .into_future()
        .map(move |aliases| RedeemableAliases::new(expected_size, aliases))
}

/// Produce hashes for a stream.
pub fn alias_stream<S>(
    expected_size: ExpectedSize,
    chunks: S,
) -> impl Future<Item = RedeemableAliases, Error = Error>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    lazy(move || {
        let mut multiplexer = Multiplexer::new();
        let aliases = add_aliases_to_multiplexer(&mut multiplexer, expected_size);

        multiplexer
            .drain(chunks)
            .map_err(|e| e.into())
            .and_then(|_| aliases.map_err(|e| e.into()))
    })
}
