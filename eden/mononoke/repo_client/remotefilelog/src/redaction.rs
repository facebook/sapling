/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use futures_old::Async;
use futures_old::Future;
use futures_old::Poll;
use mercurial_types::FileBytes;

use redactedblobstore::has_redaction_root_cause;

/// Tombstone string to replace the content of redacted files with
const REDACTED_CONTENT: &str = "PoUOK1GkdH6Xtx5j9WKYew3dZXspyfkahcNkhV6MJ4rhyNICTvX0nxmbCImFoT0oHAF9ivWGaC6ByswQZUgf1nlyxcDcahHknJS15Vl9Lvc4NokYhMg0mV1rapq1a4bhNoUI9EWTBiAkYmkadkO3YQXV0TAjyhUQWxxLVskjOwiiFPdL1l1pdYYCLTE3CpgOoxQV3EPVxGUPh1FGfk7F9Myv22qN1sUPSNN4h3IFfm2NNPRFgWPDsqAcaQ7BUSKa\n";

impl<T> RedactionFutureExt for T where T: Future {}

pub trait RedactionFutureExt: Future + Sized {
    fn rescue_redacted(self) -> RescueRedacted<Self> {
        RescueRedacted { future: self }
    }
}

#[derive(Debug)]
#[must_use = "futures do nothing unless polled"]
pub struct RescueRedacted<F> {
    future: F,
}

impl<F> Future for RescueRedacted<F>
where
    F: Future<Item = (Bytes, FileBytes), Error = Error>,
{
    type Item = (Bytes, FileBytes);
    type Error = Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match self.future.poll() {
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Ok(Async::Ready(r)) => Ok(Async::Ready(r)),
            Err(e) => {
                if has_redaction_root_cause(&e) {
                    let ret = (Bytes::new(), FileBytes(REDACTED_CONTENT.as_bytes().into()));
                    Ok(Async::Ready(ret))
                } else {
                    Err(e)
                }
            }
        }
    }
}
