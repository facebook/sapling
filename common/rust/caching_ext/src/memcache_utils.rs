// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use futures::{Future, future::ok};
use futures_ext::FutureExt;
use iobuf::IOBuf;
use memcache::MemcacheClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}};

#[derive(Clone)]
pub enum MemcacheHandler {
    Real(MemcacheClient),
    #[allow(dead_code)]
    Mock {
        data: Arc<Mutex<HashMap<String, Bytes>>>,
        get_count: Arc<AtomicUsize>,
    },
}

impl From<MemcacheClient> for MemcacheHandler {
    fn from(client: MemcacheClient) -> Self {
        MemcacheHandler::Real(client)
    }
}

impl MemcacheHandler {
    pub fn get(&self, key: String) -> impl Future<Item = Option<IOBuf>, Error = ()> {
        match self {
            MemcacheHandler::Real(ref client) => client.get(key).left_future(),
            MemcacheHandler::Mock {
                ref data,
                ref get_count,
                ..
            } => {
                get_count.fetch_add(1, Ordering::SeqCst);
                let data = data.lock().expect("poisoned lock");
                ok(data.get(&key).map(|value| value.clone().into())).right_future()
            }
        }
    }

    pub fn set(&self, key: String, value: Bytes) -> impl Future<Item = (), Error = ()> {
        match self {
            MemcacheHandler::Real(ref client) => client.set(key, value).left_future(),
            MemcacheHandler::Mock { ref data, .. } => {
                data.lock()
                    .expect("poisoned lock")
                    .insert(key.clone(), value.clone());
                ok(()).right_future()
            }
        }
    }

    #[allow(dead_code)]
    pub fn create_mock() -> Self {
        MemcacheHandler::Mock {
            data: Arc::new(Mutex::new(HashMap::new())),
            get_count: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn gets_count(&self) -> usize {
        match self {
            MemcacheHandler::Real(_) => unimplemented!(),
            MemcacheHandler::Mock { ref get_count, .. } => get_count.load(Ordering::SeqCst),
        }
    }
}
