/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::{
    CacheDisposition, CacheTtl, CachelibHandler, EntityStore, KeyedEntityStore, MemcacheEntity,
    MemcacheHandler,
};
use context::CoreContext;
use fbinit::FacebookInit;
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::ChangesetId;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::MutableRenames;

/// Bump this when code changes the layout of memcache
pub const CODEVER: u32 = 0;

#[derive(Clone)]
pub struct CacheHandlers {
    memcache: MemcacheHandler,
    presence_cachelib: CachelibHandler<HasMutableRename>,
    presence_keygen: KeyGen,
}

impl CacheHandlers {
    pub fn new(fb: FacebookInit, pool: VolatileLruCachePool) -> Result<Self, Error> {
        let memcache = MemcacheClient::new(fb)?.into();
        let presence_cachelib = pool.into();
        let presence_keygen = KeyGen::new("scm.mononoke.mutable_renames.present", CODEVER, 0); // FIXME: Sitever needs fixing up to come from tunables
        Ok(Self {
            memcache,
            presence_cachelib,
            presence_keygen,
        })
    }

    pub fn new_test() -> Self {
        let memcache = MemcacheHandler::create_mock();
        let presence_cachelib = CachelibHandler::create_mock();
        let presence_keygen = KeyGen::new("scm.mononoke.mutable_renames.present", CODEVER, 0);
        Self {
            memcache,
            presence_cachelib,
            presence_keygen,
        }
    }

    pub fn has_rename<'a>(
        &'a self,
        owner: &'a MutableRenames,
        ctx: &'a CoreContext,
    ) -> CachedHasMutableRename<'a> {
        let memcache = &self.memcache;
        let keygen = &self.presence_keygen;
        let cachelib = &self.presence_cachelib;
        CachedHasMutableRename {
            owner,
            cachelib,
            memcache,
            keygen,
            ctx,
        }
    }
}

#[derive(Abomonation, Clone, Copy, Debug)]
pub struct HasMutableRename(pub bool);
const TRUE: &[u8] = b"y";
const FALSE: &[u8] = b"n";

impl MemcacheEntity for HasMutableRename {
    fn serialize(&self) -> Bytes {
        if self.0 {
            Bytes::from_static(TRUE)
        } else {
            Bytes::from_static(FALSE)
        }
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        if bytes == TRUE {
            Ok(HasMutableRename(true))
        } else if bytes == FALSE {
            Ok(HasMutableRename(false))
        } else {
            Err(())
        }
    }
}

pub struct CachedHasMutableRename<'a> {
    owner: &'a MutableRenames,
    cachelib: &'a CachelibHandler<HasMutableRename>,
    memcache: &'a MemcacheHandler,
    keygen: &'a KeyGen,
    ctx: &'a CoreContext,
}

impl<'a> EntityStore<HasMutableRename> for CachedHasMutableRename<'a> {
    fn cachelib(&self) -> &CachelibHandler<HasMutableRename> {
        self.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        self.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        self.memcache
    }

    fn cache_determinator(&self, _v: &HasMutableRename) -> CacheDisposition {
        // A cache TTL of 4 hours means that worst case is 8 hours from making
        // a change to caches all showing it.
        //
        // Worst case is we fill memcache just before the change, giving us 4 hours
        // in memcache, then all tasks fill from memcache just before it expires,
        // giving us a further 4 hours (8 total) where all tasks have the stale data.
        CacheDisposition::Cache(CacheTtl::Ttl(Duration::from_secs(4 * 60 * 60)))
    }

    caching_ext::impl_singleton_stats!("mutable_renames.presence");
}

#[async_trait]
impl<'a> KeyedEntityStore<ChangesetId, HasMutableRename> for CachedHasMutableRename<'a> {
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        format!("mutable_renames.presence.{}", *key)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, HasMutableRename>, Error> {
        let mut res = HashMap::new();
        for key in keys {
            let has_rename = HasMutableRename(self.owner.has_rename_uncached(self.ctx, key).await?);
            res.insert(key, has_rename);
        }
        Ok(res)
    }
}
