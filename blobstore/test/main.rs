// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Tests run against all blobstore implementations.

#![deny(warnings)]
#![feature(never_type)]

#[macro_use]
extern crate error_chain;
extern crate futures;
extern crate futures_ext;
#[macro_use]
extern crate lazy_static;
extern crate tempdir;
extern crate tokio_core;

extern crate blobstore;
extern crate fileblob;
extern crate memblob;
extern crate rocksblob;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::Future;
use futures::future::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use tempdir::TempDir;
use tokio_core::reactor::{Core, Remote};

use blobstore::{Blobstore, RetryingBlobstore};
use fileblob::Fileblob;
use memblob::Memblob;
use rocksblob::Rocksblob;

mod errors {
    error_chain! {
        links {
            Fileblob(::fileblob::Error, ::fileblob::ErrorKind);
            Rocksblob(::rocksblob::Error, ::rocksblob::ErrorKind);
        }
    }

    impl From<!> for Error {
        fn from(_t: !) -> Error {
            unreachable!("! can never be instantiated")
        }
    }
}
pub use errors::*;

fn simple<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
    B::ValueIn: From<&'static [u8]>,
{
    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), b"bar"[..].into())
        .and_then(|_| blobstore.get(&foo));
    let out = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(out.as_ref(), b"bar".as_ref());
}

fn missing<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
{
    let res = blobstore.get(&"missing".to_string());
    let out = res.wait().expect("get failed");

    assert!(out.is_none());
}

fn boxable<B>(blobstore: B)
where
    B: Blobstore<Key = String>,
    B::ValueIn: From<&'static [u8]>,
    Vec<u8>: From<B::ValueOut>,
    Error: From<B::Error>,
{
    let blobstore = blobstore.boxed::<_, _, Error>();

    let foo = "foo".to_string();
    let res = blobstore
        .put(foo.clone(), b"bar".as_ref())
        .and_then(|_| blobstore.get(&foo));
    let out: Vec<u8> = res.wait().expect("pub/get failed").expect("missing");

    assert_eq!(&*out, b"bar".as_ref());
}

macro_rules! blobstore_test_impl {
    ($mod_name: ident => {
        state: $state: expr,
        new: $new_cb: expr,
        persistent: $persistent: expr,
    }) => {
        mod $mod_name {
            use super::*;

            #[test]
            fn test_simple() {
                let state = $state;
                simple($new_cb(&state));
            }

            #[test]
            fn test_missing() {
                let state = $state;
                missing($new_cb(&state));
            }

            #[test]
            fn test_boxable() {
                let state = $state;
                boxable($new_cb(&state));
            }
        }
    }
}

blobstore_test_impl! {
    memblob_test => {
        state: (),
        new: |_| Memblob::new(),
        persistent: false,
    }
}

blobstore_test_impl! {
    fileblob_test => {
        state: TempDir::new("fileblob_test").unwrap(),
        new: |dir| Fileblob::<_, Vec<u8>>::open(dir).unwrap(),
        persistent: true,
    }
}

blobstore_test_impl! {
    rocksblob_test => {
        state: TempDir::new("rocksblob_test").unwrap(),
        // create/open may need to be unified once persistence tests are added
        new: |dir| Rocksblob::create(dir).unwrap(),
        persistent: true,
    }
}

mod flaky_errors {
    error_chain! {
        errors {
            Flakiness {
                description("flakiness happend")
            }
        }
        foreign_links {
            Io(::std::io::Error);
            Oneshot(::futures::sync::oneshot::Canceled);
        }
    }
}

use flaky_errors::{Error as FlakyError, ErrorKind as FlakyErrorKind};

struct FlakyBlobstore {
    blobstore: Memblob,
    flakiness: Mutex<usize>, // number of calls that will fail
}

impl Blobstore for FlakyBlobstore {
    type Key = String;
    type ValueIn = Vec<u8>;
    type ValueOut = Self::ValueIn;
    type Error = FlakyError;
    type PutBlob = BoxFuture<(), Self::Error>;
    type GetBlob = BoxFuture<Option<Self::ValueOut>, Self::Error>;

    fn put(&self, k: Self::Key, v: Self::ValueIn) -> Self::PutBlob {
        let mut flakiness = self.flakiness.lock().expect("lock poison");
        if *flakiness == 0 {
            self.blobstore
                .put(k, v)
                .map_err(|_| FlakyError::from("never happens"))
                .boxify()
        } else {
            *flakiness = (*flakiness) - 1;
            Err(FlakyErrorKind::Flakiness.into()).into_future().boxify()
        }
    }

    fn get(&self, k: &Self::Key) -> Self::GetBlob {
        let mut flakiness = self.flakiness.lock().expect("lock poison");
        if *flakiness == 0 {
            self.blobstore
                .get(k)
                .map_err(|_| FlakyError::from("never happens"))
                .boxify()
        } else {
            *flakiness = *flakiness - 1;
            Err(FlakyErrorKind::Flakiness.into()).into_future().boxify()
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;

    use std::sync::mpsc::channel;

    lazy_static! {
        static ref REMOTE: Remote = {
            let (tx, rx) = channel();
            ::std::thread::spawn(move || {
                let mut core = Core::new().expect("failed to create tokio Core");
                tx.send(core.remote()).unwrap();
                loop {
                    core.turn(None);
                }
            });
            rx.recv().unwrap()
        };
    }

    fn flaky_blobstore(
        flakiness: usize,
        max_attempts: usize,
    ) -> RetryingBlobstore<String, Vec<u8>, Vec<u8>, FlakyError, FlakyError> {
        let blobstore = FlakyBlobstore {
            blobstore: Memblob::new(),
            flakiness: Mutex::new(flakiness),
        };
        let retry_delay = Arc::new(move |attempt| if attempt < max_attempts {
            Some(Duration::from_secs(0))
        } else {
            None
        });
        RetryingBlobstore::new(
            blobstore.arced(),
            &*REMOTE,
            retry_delay.clone(),
            retry_delay,
        )
    }

    fn check_failing(flakiness: usize, max_attempts: usize) {
        let blobstore = flaky_blobstore(flakiness, max_attempts);
        let foo = "foo".to_owned();
        let bar = b"bar"[..].into();
        match blobstore
            .put(foo.clone(), bar)
            .wait()
            .expect_err("error expected")
        {
            FlakyError(FlakyErrorKind::Flakiness, _) => (),
            err => panic!("unexpected error: {:?}", err),
        }

        let blobstore = flaky_blobstore(flakiness, max_attempts);
        match blobstore.get(&foo).wait().expect_err("error expected") {
            FlakyError(FlakyErrorKind::Flakiness, _) => (),
            err => panic!("unexpected error: {:?}", err),
        }
    }

    fn check_succeeding(flakiness: usize, max_attempts: usize) {
        let blobstore = flaky_blobstore(flakiness, max_attempts);
        let foo = "foo".to_owned();
        let bar: Vec<u8> = b"bar"[..].into();
        blobstore
            .put(foo.clone(), bar.clone())
            .wait()
            .expect("success expected");
        assert_eq!(
            blobstore.get(&foo).wait().expect("success expected"),
            Some(bar)
        );
    }

    #[test]
    fn test_one_attempt() {
        check_succeeding(0, 0);
        check_failing(1, 0);
    }

    #[test]
    fn test_multiple_attempts_succeeding() {
        check_succeeding(0, 2);
        check_succeeding(1, 2);
        check_succeeding(2, 2);
    }

    #[test]
    fn test_multiple_attempts_failing() {
        check_failing(3, 2);
        check_failing(10, 2);
        check_failing(6, 5);
    }
}
