// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use futures::{Future, Stream};

use errors::*;
use part_encode::PartEncodeBuilder;

pub fn listkey_part<N, S, K, V>(namespace: N, items: S) -> Result<PartEncodeBuilder>
where
    N: Into<Bytes>,
    S: Stream<Item = (K, V), Error = Error> + Send + 'static,
    K: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut builder = PartEncodeBuilder::mandatory("listkeys")?;
    builder.add_mparam("namespace", namespace)?;
    // Ideally we'd use a size_hint here, but streams don't appear to have one.
    let payload = Vec::with_capacity(256);
    let fut = items
        .fold(payload, |mut payload, (key, value)| {
            payload.extend_from_slice(key.as_ref());
            payload.push(b'\t');
            payload.extend_from_slice(value.as_ref());
            payload.push(b'\n');
            Ok::<_, Error>(payload)
        })
        .map_err(|err| Error::from(err.context(ErrorKind::ListkeyGeneration)));

    builder.set_data_future(fut);

    Ok(builder)
}
