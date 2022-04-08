/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::{Context, Error};
use async_trait::async_trait;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::{
    CacheDisposition, CacheTtl, CachelibHandler, EntityStore, KeyedEntityStore, MemcacheEntity,
    MemcacheHandler,
};
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{hash::Blake2, ChangesetId, MPath};
use mutable_rename_thrift as thrift;
use path_hash::PathHash;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::{MutableRenameEntry, MutableRenames};

/// Bump this when code changes the layout of memcache
pub const CODEVER: u32 = 0;

#[derive(Clone)]
pub struct CacheHandlers {
    memcache: MemcacheHandler,
    presence_cachelib: CachelibHandler<HasMutableRename>,
    presence_keygen: KeyGen,
    rename_cachelib: CachelibHandler<CachedMutableRenameEntry>,
    rename_keygen: KeyGen,
}

impl CacheHandlers {
    pub fn new(fb: FacebookInit, pool: VolatileLruCachePool) -> Result<Self, Error> {
        let memcache = MemcacheClient::new(fb)?.into();
        let presence_cachelib = pool.clone().into();
        let sitever = tunables::tunables()
            .get_mutable_renames_sitever()
            .try_into()
            .context("While converting from i64 to u32 sitever")?;
        let presence_keygen = KeyGen::new("scm.mononoke.mutable_renames.present", CODEVER, sitever);
        let rename_cachelib = pool.into();
        let rename_keygen = KeyGen::new("scm.mononoke.mutable_renames.rename", CODEVER, sitever);
        Ok(Self {
            memcache,
            presence_cachelib,
            presence_keygen,
            rename_cachelib,
            rename_keygen,
        })
    }

    pub fn new_test() -> Self {
        let memcache = MemcacheHandler::create_mock();
        let presence_cachelib = CachelibHandler::create_mock();
        let rename_cachelib = CachelibHandler::create_mock();
        let presence_keygen = KeyGen::new("scm.mononoke.mutable_renames.present", CODEVER, 0);
        let rename_keygen = KeyGen::new("scm.mononoke.mutable_renames.rename", CODEVER, 0);
        Self {
            memcache,
            presence_cachelib,
            presence_keygen,
            rename_cachelib,
            rename_keygen,
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

    pub fn get_rename<'a>(
        &'a self,
        owner: &'a MutableRenames,
        ctx: &'a CoreContext,
    ) -> CachedGetMutableRename<'a> {
        let memcache = &self.memcache;
        let keygen = &self.rename_keygen;
        let cachelib = &self.rename_cachelib;
        CachedGetMutableRename {
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

#[derive(Abomonation, Clone)]
pub struct CachedMutableRenameEntry(pub Option<CacheableMutableRenameEntry>);

#[derive(Abomonation, Clone)]
pub struct CacheableMutableRenameEntry {
    dst_cs_id: ChangesetId,
    dst_path_hash: PathHash,
    src_cs_id: ChangesetId,
    src_path: Option<Vec<u8>>,
    src_path_hash: PathHash,
    src_unode: Blake2,
    is_tree: i8,
}

impl TryFrom<CacheableMutableRenameEntry> for MutableRenameEntry {
    type Error = Error;
    fn try_from(entry: CacheableMutableRenameEntry) -> Result<Self, Error> {
        let CacheableMutableRenameEntry {
            dst_cs_id,
            dst_path_hash,
            src_cs_id,
            src_path,
            src_path_hash,
            src_unode,
            is_tree,
        } = entry;
        let src_path = src_path.as_ref().map(MPath::new).transpose()?;

        Ok(Self {
            dst_cs_id,
            dst_path_hash,
            src_cs_id,
            src_path,
            src_path_hash,
            src_unode,
            is_tree,
        })
    }
}
impl From<MutableRenameEntry> for CacheableMutableRenameEntry {
    fn from(entry: MutableRenameEntry) -> Self {
        let MutableRenameEntry {
            dst_cs_id,
            dst_path_hash,
            src_cs_id,
            src_path,
            src_path_hash,
            src_unode,
            is_tree,
        } = entry;
        let src_path = src_path.as_ref().map(MPath::to_vec);

        Self {
            dst_cs_id,
            dst_path_hash,
            src_cs_id,
            src_path,
            src_path_hash,
            src_unode,
            is_tree,
        }
    }
}

fn path_hash_to_thrift(hash: &PathHash) -> thrift::PathHash {
    let path = hash.path_bytes.0.clone();
    let is_tree = hash.is_tree;

    thrift::PathHash { path, is_tree }
}

fn path_hash_from_thrift(hash: thrift::PathHash) -> Result<PathHash, Error> {
    let path = MPath::new_opt(hash.path)?;
    Ok(PathHash::from_path_and_is_tree(path.as_ref(), hash.is_tree))
}

impl MemcacheEntity for CachedMutableRenameEntry {
    fn serialize(&self) -> Bytes {
        // Turn self into a thrift::MutableRenameEntry
        let thrift_self = match &self.0 {
            None => thrift::CachedMutableRenameEntry { entry: None },
            Some(CacheableMutableRenameEntry {
                dst_cs_id,
                dst_path_hash,
                src_cs_id,
                src_path,
                src_path_hash,
                src_unode,
                is_tree,
            }) => {
                let dst_cs_id = dst_cs_id.into_thrift();
                let dst_path_hash = path_hash_to_thrift(dst_path_hash);
                let src_cs_id = src_cs_id.into_thrift();
                let src_path = src_path.clone();
                let src_path_hash = path_hash_to_thrift(src_path_hash);
                let src_unode = src_unode.into_thrift();
                let is_tree = *is_tree;
                let entry = Some(thrift::MutableRenameEntry {
                    dst_cs_id,
                    dst_path_hash,
                    src_cs_id,
                    src_path,
                    src_path_hash,
                    src_unode,
                    is_tree,
                });
                thrift::CachedMutableRenameEntry { entry }
            }
        };
        compact_protocol::serialize(&thrift_self)
    }
    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        if let thrift::CachedMutableRenameEntry {
            entry:
                Some(thrift::MutableRenameEntry {
                    dst_cs_id,
                    dst_path_hash,
                    src_cs_id,
                    src_path,
                    src_path_hash,
                    src_unode,
                    is_tree,
                }),
        } = compact_protocol::deserialize(bytes).map_err(|_| ())?
        {
            let dst_cs_id = ChangesetId::from_thrift(dst_cs_id).map_err(|_| ())?;
            let dst_path_hash = path_hash_from_thrift(dst_path_hash).map_err(|_| ())?;
            let src_cs_id = ChangesetId::from_thrift(src_cs_id).map_err(|_| ())?;
            let src_path_hash = path_hash_from_thrift(src_path_hash).map_err(|_| ())?;
            let src_unode = Blake2::from_thrift(src_unode).map_err(|_| ())?;
            let entry = CacheableMutableRenameEntry {
                dst_cs_id,
                dst_path_hash,
                src_cs_id,
                src_path,
                src_path_hash,
                src_unode,
                is_tree,
            };
            Ok(CachedMutableRenameEntry(Some(entry)))
        } else {
            Ok(CachedMutableRenameEntry(None))
        }
    }
}

pub struct CachedGetMutableRename<'a> {
    owner: &'a MutableRenames,
    cachelib: &'a CachelibHandler<CachedMutableRenameEntry>,
    memcache: &'a MemcacheHandler,
    keygen: &'a KeyGen,
    ctx: &'a CoreContext,
}

impl<'a> EntityStore<CachedMutableRenameEntry> for CachedGetMutableRename<'a> {
    fn cachelib(&self) -> &CachelibHandler<CachedMutableRenameEntry> {
        self.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        self.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        self.memcache
    }

    fn cache_determinator(&self, _v: &CachedMutableRenameEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::Ttl(Duration::from_secs(4 * 60 * 60)))
    }

    caching_ext::impl_singleton_stats!("mutable_renames.get_rename");
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct RenameKey {
    dst_cs_id: ChangesetId,
    dst_path: Option<MPath>,
}

impl RenameKey {
    pub fn new(dst_cs_id: ChangesetId, dst_path: Option<MPath>) -> Self {
        Self {
            dst_cs_id,
            dst_path,
        }
    }
}

#[async_trait]
impl<'a> KeyedEntityStore<RenameKey, CachedMutableRenameEntry> for CachedGetMutableRename<'a> {
    fn get_cache_key(&self, key: &RenameKey) -> String {
        match &key.dst_path {
            None => format!("mutable_renames.rename.cs_id_at_root.{}", key.dst_cs_id),
            Some(path) => format!(
                "mutable_renames.rename.cs_id_and_path.{}.{}",
                key.dst_cs_id, path
            ),
        }
    }

    async fn get_from_db(
        &self,
        keys: HashSet<RenameKey>,
    ) -> Result<HashMap<RenameKey, CachedMutableRenameEntry>, Error> {
        let mut res = HashMap::new();
        // Right now, the only caller always asks for a single entry from the cache, so
        // this function is either not called, or called once.
        // If we build a batch interface, we should make this use it for batched fills
        for key in keys {
            let rename_entry = CachedMutableRenameEntry(
                self.owner
                    .get_rename_uncached(self.ctx, key.dst_cs_id, key.dst_path.clone())
                    .await?
                    .map(CacheableMutableRenameEntry::from),
            );
            res.insert(key, rename_entry);
        }
        Ok(res)
    }
}
