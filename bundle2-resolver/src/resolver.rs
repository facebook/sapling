// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::BufRead;
use std::sync::Arc;

use futures::{Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;
use tokio_io::AsyncRead;

use blobrepo::BlobRepo;
use errors::*;
use mercurial_bundles::bundle2::{self, Bundle2Stream, StreamEvent};

pub fn resolve<R>(
    _repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: Bundle2Stream<'static, R>,
) -> BoxFuture<bundle2::Remainder<R>, Error>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    info!(logger, "unbundle heads {:?}", heads);
    bundle2
        .filter_map(move |event| match event {
            StreamEvent::Done(remainder) => Some(remainder),
            StreamEvent::Next(item) => {
                debug!(logger, "bundle2 item: {:?}", item);
                None
            }
        })
        .into_future()
        .map(|(remainder, _)| remainder.expect("No remainder left"))
        .map_err(|(err, _)| err)
        .boxify()
}
