// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use failure_ext::Result;
use futures::{
    future::{lazy, IntoFuture},
    Future, Stream,
};
use mononoke_types::hash;

use crate::expected_size::ExpectedSize;
use crate::streamhash::hash_stream;

use crate::incremental_hash::{
    GitSha1IncrementalHasher, Sha1IncrementalHasher, Sha256IncrementalHasher,
};

type Aliases = (hash::Sha1, hash::Sha256, hash::GitSha1);

/// Hashes are dependent on the ExpectedSize we received, so we want to make sure callers verify
/// that the size they gave us is actually the one they observed before giving them access to the
/// resulting hashes. To do so, we require callers to provide the effective size to retrieve
/// aliases.
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

/// Produce hashes for a stream.
pub fn alias_stream<E, S>(
    expected_size: ExpectedSize,
    chunks: S,
) -> impl Future<Item = RedeemableAliases, Error = E> + Send
where
    E: Send,
    S: Stream<Item = Bytes, Error = E> + Send + 'static,
{
    lazy(move || {
        // Split out the error to not require cloning it.
        let (chunks, err) = futures_ext::split_err(chunks);

        // One stream for the data itself, and one for each hash format we might need
        let mut copies = futures_ext::stream_clone(chunks, 3).into_iter();

        // It's safe to unwrap copies.next() below because we make enough copies (and we didn't,
        // we'd hit the issue deterministically in tests).
        let sha1 = hash_stream(Sha1IncrementalHasher::new(), copies.next().unwrap());
        let sha256 = hash_stream(Sha256IncrementalHasher::new(), copies.next().unwrap());
        let git_sha1 = hash_stream(
            GitSha1IncrementalHasher::new(expected_size),
            copies.next().unwrap(),
        );
        assert!(copies.next().is_none());

        let res = (sha1, sha256, git_sha1).into_future();

        // Rejoin error with output stream.
        res.map_err(|e| -> E { e })
            .select(err.map(|e| -> Aliases { e }))
            .map(|(res, _)| res)
            .map_err(|(err, _)| err)
            .map(move |aliases| RedeemableAliases::new(expected_size, aliases))
    })
}
