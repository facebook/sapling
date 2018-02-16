// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::io::BufRead;
use std::sync::Arc;

use futures::{Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use slog::Logger;
use tokio_io::AsyncRead;

use blobrepo::BlobRepo;
use changegroup::{convert_to_revlog_changesets, split_changegroup};
use errors::*;
use mercurial_bundles::Bundle2Item;
use mercurial_bundles::bundle2::{self, Bundle2Stream, StreamEvent};

pub fn resolve<R>(
    _repo: Arc<BlobRepo>,
    logger: Logger,
    heads: Vec<String>,
    bundle2: Bundle2Stream<R>,
) -> BoxFuture<bundle2::Remainder<R>, Error>
where
    R: AsyncRead + BufRead + 'static + Send,
{
    info!(logger, "unbundle heads {:?}", heads);
    bundle2
        .and_then(move |event| match event {
            StreamEvent::Next(item) => {
                debug!(logger, "bundle2 item: {:?}", item);
                match item {
                    Bundle2Item::Start(_) => Ok(None).into_future().boxify(),
                    Bundle2Item::Changegroup(_, parts) => {
                        let (c, f) = split_changegroup(parts);
                        convert_to_revlog_changesets(c)
                            .for_each({
                                let logger = logger.clone();
                                move |p| {
                                    debug!(logger, "changegroup part: {:?}", p);
                                    Ok(())
                                }
                            })
                            .join(f.for_each({
                                let logger = logger.clone();
                                move |p| {
                                    debug!(logger, "changegroup part: {:?}", p);
                                    Ok(())
                                }
                            }))
                            .map(|((), ())| None)
                            .boxify()
                    }
                    Bundle2Item::B2xTreegroup2(_, parts) => parts
                        .for_each({
                            let logger = logger.clone();
                            move |p| {
                                debug!(logger, "b2xtreegroup2 part: {:?}", p);
                                Ok(())
                            }
                        })
                        .map(|()| None)
                        .boxify(),
                    Bundle2Item::Replycaps(_, part) => part.map({
                        let logger = logger.clone();
                        move |p| {
                            debug!(logger, "replycaps part: {:?}", p);
                            None
                        }
                    }).boxify(),
                }
            }
            StreamEvent::Done(remainder) => Ok(Some(remainder)).into_future().boxify(),
        })
        .filter_map(|x| x)
        .into_future()
        .map(|(remainder, _)| remainder.expect("No remainder left"))
        .map_err(|(err, _)| err)
        .boxify()
}
