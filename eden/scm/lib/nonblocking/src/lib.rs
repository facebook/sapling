/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Turn an async function to a sync non-blocking function.

use std::future::Future;
use std::io;
use std::task::Context;
use std::task::Poll;
use std::task::RawWaker;
use std::task::RawWakerVTable;
use std::task::Waker;

/// Attempt to resolve a `future` without blocking.
/// Return `WouldBlock` error if the future will block.
/// Return the resolved value otherwise.
pub fn non_blocking<F, R>(future: F) -> io::Result<R>
where
    F: Future<Output = R>,
{
    let waker = waker();
    let mut cx = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(result) => Ok(result),
        Poll::Pending => Err(io::ErrorKind::WouldBlock.into()),
    }
}

/// Similar to `non_blocking`, but unwraps a level of `Result`.
pub fn non_blocking_result<F, T, E>(future: F) -> Result<T, E>
where
    F: Future<Output = Result<T, E>>,
    E: From<io::Error>,
{
    non_blocking(future)?
}

fn waker() -> Waker {
    let raw_waker = clone(std::ptr::null());
    unsafe { Waker::from_raw(raw_waker) }
}

fn vtable() -> &'static RawWakerVTable {
    &RawWakerVTable::new(clone, noop, noop, noop)
}

fn clone(data: *const ()) -> RawWaker {
    RawWaker::new(data, vtable())
}

fn noop(_: *const ()) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_blocking_ok() {
        async fn f() -> usize {
            g().await + 4
        }

        async fn g() -> usize {
            3
        }

        async fn h() -> io::Result<usize> {
            Ok(5)
        }

        assert_eq!(non_blocking(async { f().await }).unwrap(), 7);
        assert_eq!(non_blocking_result(h()).unwrap(), 5);
    }

    #[test]
    fn test_non_blocking_err() {
        let (sender, receiver) = futures::channel::oneshot::channel::<usize>();
        assert_eq!(
            non_blocking(async { receiver.await }).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        drop(sender);
    }
}
