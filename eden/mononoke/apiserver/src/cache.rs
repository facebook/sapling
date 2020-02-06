/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use bytes::Bytes;
use fbinit::FacebookInit;
use futures::{Future, IntoFuture};

use caching_ext::{CachelibHandler, MemcacheHandler};
use cloned::cloned;
use futures_ext::FutureExt;
use memcache::{KeyGen, MemcacheClient};

use crate::errors::ErrorKind;

#[derive(Clone)]
pub struct CacheManager {
    memcache: MemcacheHandler,
    cachelib: CachelibHandler<Vec<u8>>,
    keygen: KeyGen,
}

impl CacheManager {
    const KEY_PREFIX: &'static str = "scm.mononoke.apiserver";
    const MC_CODEVER: u32 = 1;
    const MC_SITEVER: u32 = 1;

    pub fn new(fb: FacebookInit) -> Result<Self, ErrorKind> {
        let cachelib = match cachelib::get_volatile_pool("content-sha1") {
            Ok(Some(e)) => Ok(e),
            _ => Err(ErrorKind::InternalError(Error::msg(
                "Failed to get cachelib cache",
            ))),
        }?;

        Ok(CacheManager {
            memcache: MemcacheClient::new(fb).into(),
            cachelib: cachelib.into(),
            keygen: KeyGen::new(Self::KEY_PREFIX, Self::MC_CODEVER, Self::MC_SITEVER),
        })
    }

    #[cfg(test)]
    pub fn create_mock() -> Self {
        CacheManager {
            memcache: MemcacheHandler::create_mock(),
            cachelib: CachelibHandler::create_mock(),
            keygen: KeyGen::new(Self::KEY_PREFIX, Self::MC_CODEVER, Self::MC_SITEVER),
        }
    }

    fn get(&self, key: String) -> impl Future<Item = Bytes, Error = ()> {
        let key = self.keygen.key(key);

        if let Ok(Some(cached)) = self.cachelib.get_cached(&key) {
            Ok(cached).into_future().map(Bytes::from).left_future()
        } else {
            self.memcache
                .get(key.clone())
                .and_then(|result| match result {
                    Some(cached) => Ok(Bytes::from(cached)),
                    None => Err(()),
                })
                .map({
                    cloned!(self.cachelib);
                    move |cached| {
                        let _ = cachelib.set_cached(&key, &cached.to_vec());
                        cached
                    }
                })
                .right_future()
        }
    }

    fn set(&self, key: String, value: Bytes) -> impl Future<Item = (), Error = ()> {
        let key = self.keygen.key(key);
        let _ = self.cachelib.set_cached(&key, &value.to_vec());
        self.memcache.set(key, value)
    }

    pub fn get_or_fill<
        RES: Future<Item = ITEM, Error = ErrorKind> + Send + 'static,
        ITEM: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    >(
        &self,
        key: String,
        fill: RES,
    ) -> impl Future<Item = ITEM, Error = ErrorKind> {
        let fill_future = fill.and_then({
            cloned!(key, self as this);
            move |resp| {
                bincode::serialize(&resp)
                    .map_err(|_| ())
                    .into_future()
                    .and_then(move |serialized| this.set(key, serialized.into()))
                    .then(|_| Ok(resp))
            }
        });

        self.get(key)
            .and_then(|cached| bincode::deserialize(&cached[..]).map_err(|_| ()))
            .or_else(|_| fill_future)
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
        let manager = CacheManager::create_mock();
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        let key = "key-test-missed".to_string();
        let keygen_key = manager.keygen.key(key.clone());
        let num = 1032;
        let value = CacheableStruct(num);
        let serialized = bincode::serialize(&value).unwrap();

        let result = runtime.block_on(manager.get(key.clone()));

        // not in cache yet
        assert!(result.is_err());

        let result = runtime
            .block_on(manager.get_or_fill(key.clone(), Ok(value).into_future()))
            .unwrap();
        assert_eq!(result.0, num);

        let result = runtime.block_on(manager.get(key.clone())).unwrap();
        let result: CacheableStruct = bincode::deserialize(&result[..]).unwrap();
        assert_eq!(result.0, num);

        let result = manager.cachelib.get_cached(&keygen_key).unwrap().unwrap();
        assert_eq!(result, serialized);

        let result = runtime
            .block_on(manager.memcache.get(keygen_key))
            .unwrap()
            .unwrap();
        assert_eq!(Bytes::from(result), serialized);
    }

    #[test]
    fn test_memcache_missed() {
        let manager = CacheManager::create_mock();
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        let key = "key-test-memcache-missed".to_string();
        let keygen_key = manager.keygen.key(key.clone());
        let num = 1032;
        let value = CacheableStruct(num);
        let serialized = bincode::serialize(&value).unwrap();

        manager.cachelib.set_cached(&keygen_key, &serialized).ok();

        let result = runtime
            .block_on(manager.memcache.get(keygen_key.clone()))
            .unwrap();
        assert_eq!(result, None);

        let result = runtime
            .block_on(manager.get_or_fill(
                key.clone(),
                lazy(|| -> FutureResult<CacheableStruct, ErrorKind> { unreachable!() }),
            ))
            .unwrap();
        assert_eq!(result.0, num);

        let result = runtime.block_on(manager.get(key.clone())).unwrap();
        let result: CacheableStruct = bincode::deserialize(&result[..]).unwrap();
        assert_eq!(result.0, num);

        // memcache is not filled when the key is present in cachelib
        let result = runtime.block_on(manager.memcache.get(keygen_key)).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_cachelib_missed() {
        let manager = CacheManager::create_mock();
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

        let key = "key-test-cachelib-missed".to_string();
        let keygen_key = manager.keygen.key(key.clone());
        let num = 1032;
        let value = CacheableStruct(num);
        let serialized = bincode::serialize(&value).unwrap();
        let serialized_bytes = Bytes::from(serialized.clone());

        runtime
            .block_on(manager.memcache.set(keygen_key.clone(), serialized_bytes))
            .unwrap();

        let result = manager.cachelib.get_cached(&keygen_key).unwrap();
        assert_eq!(result, None);

        let result = runtime
            .block_on(manager.get_or_fill(
                key.clone(),
                lazy(|| -> FutureResult<CacheableStruct, ErrorKind> { unreachable!() }),
            ))
            .unwrap();
        assert_eq!(result.0, num);

        let result = runtime.block_on(manager.get(key.clone())).unwrap();
        let result: CacheableStruct = bincode::deserialize(&result[..]).unwrap();
        assert_eq!(result.0, num);

        // cachelib is filled when data is fetched from memcache
        let result = manager.cachelib.get_cached(&keygen_key).unwrap().unwrap();
        assert_eq!(result, serialized);
    }

    #[test]
    fn test_cache_hit() {
        let manager = CacheManager::create_mock();
        let mut runtime = tokio_compat::runtime::Runtime::new().unwrap();

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
