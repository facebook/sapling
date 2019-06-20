// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use bytes::Bytes;
use futures::{Future, IntoFuture};

use caching_ext::MemcacheHandler;
use cloned::cloned;
use futures_ext::{BoxFuture, FutureExt};
use iobuf::IOBuf;
use memcache::{KeyGen, MemcacheClient};

use crate::errors::ErrorKind;

#[derive(Clone)]
pub struct CacheManager {
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl CacheManager {
    const KEY_PREFIX: &'static str = "scm.mononoke.apiserver";
    const MC_CODEVER: u32 = 1;
    const MC_SITEVER: u32 = 1;

    pub fn new() -> Self {
        CacheManager {
            memcache: MemcacheClient::new().into(),
            keygen: KeyGen::new(Self::KEY_PREFIX, Self::MC_CODEVER, Self::MC_SITEVER),
        }
    }

    #[cfg(test)]
    pub fn new_with_memcache(memcache: MemcacheHandler) -> Self {
        CacheManager {
            memcache,
            keygen: KeyGen::new(Self::KEY_PREFIX, Self::MC_CODEVER, Self::MC_SITEVER),
        }
    }

    fn get(&self, key: String) -> impl Future<Item = Option<IOBuf>, Error = ()> {
        self.memcache.get(self.keygen.key(key))
    }

    fn set(&self, key: String, value: Bytes) -> impl Future<Item = (), Error = ()> {
        self.memcache.set(self.keygen.key(key), value)
    }

    #[allow(dead_code)]
    pub fn get_or_fill<
        RES: Future<Item = ITEM, Error = ErrorKind> + Send + 'static,
        ITEM: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    >(
        &self,
        key: String,
        fill: RES,
    ) -> BoxFuture<ITEM, ErrorKind> {
        let fill_future = fill.and_then({
            let this = self.clone();
            cloned!(key);
            move |resp| {
                bincode::serialize(&resp)
                    .map_err(|_| ())
                    .into_future()
                    .and_then(move |serialized| this.set(key, serialized.into()))
                    .then(|_| Ok(resp))
            }
        });

        self.get(key)
            .and_then(|result| match result {
                Some(cached) => Ok(Bytes::from(cached)),
                None => Err(()),
            })
            .and_then(|cached| bincode::deserialize(&cached[..]).map_err(|_| ()))
            .or_else(|_| fill_future)
            .boxify()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use futures::future::{lazy, FutureResult};
    use serde_derive::{Deserialize, Serialize};

    #[derive(Deserialize, Serialize, Debug)]
    struct CacheableStruct(pub u32);

    #[test]
    fn test_cache_missed() {
        let mock_memcache = MemcacheHandler::create_mock();
        let manager = CacheManager::new_with_memcache(mock_memcache);
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let key = "key-test-missed".to_string();
        let num = 1032;
        let value = CacheableStruct(num);

        let result = runtime.block_on(manager.get(key.clone())).unwrap();

        // not in cache yet
        assert_eq!(result, None);

        let result = runtime
            .block_on(manager.get_or_fill(key.clone(), Ok(value).into_future()))
            .unwrap();
        assert_eq!(result.0, num);

        let result = runtime.block_on(manager.get(key.clone())).unwrap().unwrap();
        let result = Bytes::from(result);
        let result: CacheableStruct = bincode::deserialize(&result[..]).unwrap();
        assert_eq!(result.0, num);
    }

    #[test]
    fn test_cache_hit() {
        let mock_memcache = MemcacheHandler::create_mock();
        let manager = CacheManager::new_with_memcache(mock_memcache);
        let mut runtime = tokio::runtime::Runtime::new().unwrap();

        let key = "key-test-hit".to_string();
        let num = 1032;
        let value = CacheableStruct(num);

        let result = runtime
            .block_on(manager.get_or_fill(key.clone(), Ok(value).into_future()))
            .unwrap();
        assert_eq!(result.0, num);

        let result = runtime
            .block_on(manager.get_or_fill(
                key.clone(),
                lazy(|| -> FutureResult<CacheableStruct, ErrorKind> { unreachable!() }),
            ))
            .unwrap();
        assert_eq!(result.0, num);
    }
}
