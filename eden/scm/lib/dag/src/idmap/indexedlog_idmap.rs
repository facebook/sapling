/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::fs::File;
use std::fs::{self};
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;

use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use fs2::FileExt;
use indexedlog::log;

use super::IdMapWrite;
use crate::errors::bug;
use crate::errors::programming;
use crate::errors::NotFoundError;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::ops::IdConvert;
use crate::ops::Persist;
use crate::ops::PrefixLookup;
use crate::ops::TryClone;
use crate::Result;
use crate::VerLink;

/// Bi-directional mapping between an integer id and a name (`[u8]`).
///
/// Backed by the filesystem.
pub struct IdMap {
    pub(crate) log: log::Log,
    path: PathBuf,
    need_rebuild_non_master: bool,
    map_id: String,
    map_version: VerLink,
}

impl IdMap {
    const INDEX_ID_TO_NAME: usize = 0;
    const INDEX_GROUP_NAME_TO_ID: usize = 1;

    /// Magic bytes in `Log` that indicates "remove all non-master id->name
    /// mappings". A valid entry has at least 8 bytes so does not conflict
    /// with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    /// Start offset in an entry for "name".
    const NAME_OFFSET: usize = 8 + Group::BYTES;

    /// Create an [`IdMap`] backed by the given directory.
    ///
    /// By default, only read-only operations are allowed. For writing
    /// access, call [`IdMap::make_writable`] to get a writable instance.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let log = Self::log_open_options().open(path)?;
        Self::open_from_log(log)
    }
}

impl TryClone for IdMap {
    fn try_clone(&self) -> Result<Self> {
        let result = Self {
            log: self.log.try_clone()?,
            path: self.path.clone(),
            need_rebuild_non_master: self.need_rebuild_non_master,
            map_id: self.map_id.clone(),
            map_version: self.map_version.clone(),
        };
        Ok(result)
    }
}

impl IdMap {
    pub(crate) fn open_from_log(log: log::Log) -> Result<Self> {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        let map_id = format!("ilog:{}", path.display());
        Ok(Self {
            log,
            path,
            need_rebuild_non_master: false,
            map_id,
            map_version: VerLink::new(),
        })
    }

    pub(crate) fn log_open_options() -> log::OpenOptions {
        log::OpenOptions::new()
            .create(true)
            .index("id", |data| {
                assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < 8);
                assert!(Group::BITS == 8);
                if data.len() < 8 {
                    if data == Self::MAGIC_CLEAR_NON_MASTER {
                        vec![log::IndexOutput::RemovePrefix(Box::new([
                            Group::NON_MASTER.0 as u8,
                        ]))]
                    } else {
                        panic!("bug: invalid segment {:?}", &data);
                    }
                } else {
                    vec![log::IndexOutput::Reference(0..8)]
                }
            })
            .index("group-name", |data| {
                if data.len() >= 8 {
                    vec![log::IndexOutput::Reference(8..(data.len() as u64))]
                } else {
                    if data == Self::MAGIC_CLEAR_NON_MASTER {
                        vec![log::IndexOutput::RemovePrefix(Box::new([
                            Group::NON_MASTER.0 as u8,
                        ]))]
                    } else {
                        panic!("bug: invalid segment {:?}", &data);
                    }
                }
            })
            .flush_filter(Some(|_, _| {
                panic!("programming error: idmap changed by other process")
            }))
    }

    /// Find name by a specified integer id.
    pub fn find_name_by_id(&self, id: Id) -> Result<Option<&[u8]>> {
        let key = id.0.to_be_bytes();
        let key = self.log.lookup(Self::INDEX_ID_TO_NAME, &key)?.nth(0);
        match key {
            Some(Ok(entry)) => {
                if entry.len() < 8 {
                    return bug("index key should have 8 bytes at least");
                }
                Ok(Some(&entry[Self::NAME_OFFSET..]))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Find VertexName by a specified integer id.
    pub fn find_vertex_name_by_id(&self, id: Id) -> Result<Option<VertexName>> {
        self.find_name_by_id(id)
            .map(|v| v.map(|n| VertexName(self.log.slice_to_bytes(n))))
    }

    /// Find the integer id matching the given name.
    pub fn find_id_by_name(&self, name: &[u8]) -> Result<Option<Id>> {
        for group in Group::ALL.iter() {
            let mut group_name = Vec::with_capacity(Group::BYTES + name.len());
            group_name.extend_from_slice(&group.bytes());
            group_name.extend_from_slice(name);
            let key = self
                .log
                .lookup(Self::INDEX_GROUP_NAME_TO_ID, group_name)?
                .nth(0);
            match key {
                Some(Ok(mut entry)) => {
                    if entry.len() < 8 {
                        return bug("index key should have 8 bytes at least");
                    }
                    let id = Id(entry.read_u64::<BigEndian>().unwrap());
                    return Ok(Some(id));
                }
                None => {}
                Some(Err(err)) => return Err(err.into()),
            }
        }
        Ok(None)
    }

    /// Similar to `find_name_by_id`, but returns None if group > `max_group`.
    pub fn find_id_by_name_with_max_group(
        &self,
        name: &[u8],
        max_group: Group,
    ) -> Result<Option<Id>> {
        Ok(self.find_id_by_name(name)?.and_then(|id| {
            if id.group() <= max_group {
                Some(id)
            } else {
                None
            }
        }))
    }

    /// Insert a new entry mapping from a name to an id.
    ///
    /// Errors if the new entry conflicts with existing entries.
    pub fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        let group = id.group();
        let existing_name = self.find_name_by_id(id)?;
        if let Some(existing_name) = existing_name {
            if existing_name == name {
                return Ok(());
            } else {
                return bug(format!(
                    "new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                    id, name, id, existing_name
                ));
            }
        }
        let existing_id = self.find_id_by_name(name)?;
        if let Some(existing_id) = existing_id {
            // Allow re-assigning Ids from a higher group to a lower group.
            // For example, when a non-master commit gets merged into the
            // master branch, the id is re-assigned to master. But, the
            // ids in the master group will never be re-assigned to
            // non-master groups.
            if existing_id == id {
                return Ok(());
            } else if existing_id.group() <= group {
                return bug(format!(
                    "new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                    id, name, existing_id, name
                ));
            }
            tracing::debug!("need reassign {:?} {:?} => {:?}", name, existing_id, id);
            // Mark "need_rebuild_non_master". This prevents "sync" until
            // the callsite uses "remove_non_master" to remove and re-insert
            // non-master ids.
            self.need_rebuild_non_master = true;
        }

        let mut data = Vec::with_capacity(8 + Group::BYTES + name.len());
        data.extend_from_slice(&id.0.to_be_bytes());
        data.extend_from_slice(&id.group().bytes());
        data.extend_from_slice(&name);
        self.log.append(data)?;
        self.map_version.bump();
        Ok(())
    }

    /// Lookup names by hex prefix.
    fn find_names_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>> {
        let mut result = Vec::with_capacity(limit);
        for group in Group::ALL.iter().rev() {
            let mut prefix = Vec::with_capacity(Group::BYTES * 2 + hex_prefix.len());
            prefix.extend_from_slice(&group.hex_bytes());
            prefix.extend_from_slice(hex_prefix);
            for entry in self
                .log
                .lookup_prefix_hex(Self::INDEX_GROUP_NAME_TO_ID, prefix)?
            {
                let (k, _v) = entry?;
                let vertex = VertexName(self.log.slice_to_bytes(&k[Group::BYTES..]));
                if !result.contains(&vertex) {
                    result.push(vertex);
                }
                if result.len() >= limit {
                    return Ok(result);
                }
            }
        }
        Ok(result)
    }
}

impl fmt::Debug for IdMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IdMap {{\n")?;
        for data in self.log.iter() {
            if let Ok(mut data) = data {
                let id = data.read_u64::<BigEndian>().unwrap();
                let _group = data.read_u8().unwrap();
                let mut name = Vec::with_capacity(20);
                data.read_to_end(&mut name).unwrap();
                let name = if name.len() >= 20 {
                    VertexName::from(name).to_hex()
                } else {
                    String::from_utf8_lossy(&name).to_string()
                };
                let id = Id(id);
                write!(f, "  {}: {},\n", name, id)?;
            }
        }
        write!(f, "}}\n")?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl IdConvert for IdMap {
    async fn vertex_id(&self, name: VertexName) -> Result<Id> {
        self.find_id_by_name(name.as_ref())?
            .ok_or_else(|| name.not_found_error())
    }
    async fn vertex_id_with_max_group(
        &self,
        name: &VertexName,
        max_group: Group,
    ) -> Result<Option<Id>> {
        self.find_id_by_name_with_max_group(name.as_ref(), max_group)
    }
    async fn vertex_name(&self, id: Id) -> Result<VertexName> {
        self.find_vertex_name_by_id(id)?
            .ok_or_else(|| id.not_found_error())
    }
    async fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        Ok(self.find_id_by_name(name.as_ref())?.is_some())
    }
    async fn contains_vertex_id_locally(&self, ids: &[Id]) -> Result<Vec<bool>> {
        let mut list = Vec::with_capacity(ids.len());
        for &id in ids {
            list.push(self.find_name_by_id(id)?.is_some());
        }
        Ok(list)
    }
    async fn contains_vertex_name_locally(&self, names: &[VertexName]) -> Result<Vec<bool>> {
        let mut list = Vec::with_capacity(names.len());
        for name in names {
            let contains = self.find_id_by_name(name.as_ref())?.is_some();
            tracing::trace!("contains_vertex_name_locally({:?}) = {}", name, contains);
            list.push(contains);
        }
        Ok(list)
    }
    fn map_id(&self) -> &str {
        &self.map_id
    }
    fn map_version(&self) -> &VerLink {
        &self.map_version
    }
}

#[async_trait::async_trait]
impl IdMapWrite for IdMap {
    async fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        IdMap::insert(self, id, name)
    }
    async fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(IdMap::MAGIC_CLEAR_NON_MASTER)?;
        self.map_version = VerLink::new();
        self.need_rebuild_non_master = false;
        Ok(())
    }
    async fn need_rebuild_non_master(&self) -> bool {
        self.need_rebuild_non_master
    }
}

impl Persist for IdMap {
    type Lock = File;

    fn lock(&mut self) -> Result<Self::Lock> {
        if self.log.iter_dirty().next().is_some() {
            return programming("lock() must be called without dirty in-memory entries");
        }
        let lock_file = {
            let mut path = self.path.clone();
            path.push("wlock");
            File::open(&path).or_else(|_| {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
            })?
        };
        lock_file.lock_exclusive()?;
        Ok(lock_file)
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        if self.need_rebuild_non_master {
            return bug("cannot persist with re-assigned ids unresolved");
        }
        self.log.sync()?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl PrefixLookup for IdMap {
    async fn vertexes_by_hex_prefix(
        &self,
        hex_prefix: &[u8],
        limit: usize,
    ) -> Result<Vec<VertexName>> {
        self.find_names_by_hex_prefix(hex_prefix, limit)
    }
}
