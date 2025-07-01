/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::fs;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use indexedlog::log;
use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use super::IdMapWrite;
use super::mem_idmap::CoreMemIdMap;
use crate::Result;
use crate::VerLink;
use crate::errors::NotFoundError;
use crate::errors::bug;
use crate::errors::programming;
use crate::id::Group;
use crate::id::Id;
use crate::id::Vertex;
use crate::ops::IdConvert;
use crate::ops::Persist;
use crate::ops::PrefixLookup;
use crate::ops::TryClone;

/// Bi-directional mapping between an integer id and a name (`[u8]`).
///
/// Backed by the filesystem.
pub struct IdMap {
    pub(crate) log: log::Log,
    path: PathBuf,
    map_id: String,
    map_version: VerLink,
    // Extra state for ids in the VIRTUAL group. Never write to disk.
    virtual_map: CoreMemIdMap,
}

impl IdMap {
    // Format:
    //
    // - Insertion:
    //   id (8 bytes, BE) + group (1 byte) + name (n bytes)
    // - Deletion:
    //   u64::MAX (8 bytes, BE) + n (VLQ) + [id (VLQ) + len(name) (VLQ) + name ] * n
    // - Clear non-master (only id->name mappings, being deprecated):
    //   CLRNM

    const INDEX_ID_TO_NAME: usize = 0;
    const INDEX_GROUP_NAME_TO_ID: usize = 1;

    /// Magic bytes in `Log` that indicates "remove all non-master id->name
    /// mappings". A valid entry has at least 8 bytes so does not conflict
    /// with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    /// Magic prefix for deletion. It's u64::MAX id, which does not conflict
    /// with a valid id because it's > `Id::MAX`.
    const MAGIC_DELETION_PREFIX: &'static [u8] = &u64::MAX.to_be_bytes();

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
            map_id: self.map_id.clone(),
            map_version: self.map_version.clone(),
            virtual_map: self.virtual_map.clone(),
        };
        Ok(result)
    }
}

impl IdMap {
    pub(crate) fn open_from_log(log: log::Log) -> Result<Self> {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        let map_id = format!("ilog:{}", path.display());
        let map_version = VerLink::from_storage_version_or_new(&map_id, log.version());
        Ok(Self {
            log,
            path,
            map_id,
            map_version,
            virtual_map: Default::default(),
        })
    }

    pub(crate) fn log_open_options() -> log::OpenOptions {
        assert!(Self::MAGIC_DELETION_PREFIX > &Id::MAX.0.to_be_bytes()[..]);
        log::OpenOptions::new()
            .create(true)
            .index("id", |data| {
                assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < 8);
                assert!(Group::BITS == 8);
                if data.starts_with(Self::MAGIC_DELETION_PREFIX) {
                    let items =
                        decode_deletion_entry(data).expect("deletion entry should be valid");
                    items
                        .into_iter()
                        .map(|(id, _name)| log::IndexOutput::Remove(id.0.to_be_bytes().into()))
                        .collect()
                } else if data.len() < 8 {
                    if data == Self::MAGIC_CLEAR_NON_MASTER {
                        vec![log::IndexOutput::RemovePrefix(Box::new([
                            Group::NON_MASTER.0 as u8,
                        ]))]
                    } else {
                        panic!("bug: invalid segment {:?}", &data);
                    }
                } else {
                    let slice = &data[..8];
                    let slice: [u8; 8] = slice.try_into().unwrap(); // length checked
                    let id = Id(u64::from_be_bytes(slice));
                    assert!(
                        !id.is_virtual(),
                        "bug: VIRTUAL group should never be written to disk"
                    );
                    vec![log::IndexOutput::Reference(0..8)]
                }
            })
            .index("group-name", |data| {
                if data.starts_with(Self::MAGIC_DELETION_PREFIX) {
                    let items =
                        decode_deletion_entry(data).expect("deletion entry should be valid");
                    items
                        .into_iter()
                        .map(|(id, name)| {
                            let mut key = Vec::with_capacity(name.len() + 1);
                            key.extend_from_slice(&id.group().bytes());
                            key.extend_from_slice(name);
                            log::IndexOutput::Remove(key.into())
                        })
                        .collect()
                } else if data.len() >= 8 {
                    vec![log::IndexOutput::Reference(8..(data.len() as u64))]
                } else if data == Self::MAGIC_CLEAR_NON_MASTER {
                    vec![log::IndexOutput::RemovePrefix(Box::new([
                        Group::NON_MASTER.0 as u8,
                    ]))]
                } else {
                    panic!("bug: invalid segment {:?}", &data);
                }
            })
            .flush_filter(Some(|_, _| {
                panic!("programming error: idmap changed by other process")
            }))
    }

    /// Find name by a specified integer id.
    pub fn find_name_by_id(&self, id: Id) -> Result<Option<&[u8]>> {
        if id.is_virtual() {
            return Ok(self.virtual_map.lookup_vertex_name(id).map(|v| v.as_ref()));
        }
        let key = id.0.to_be_bytes();
        let key = self.log.lookup(Self::INDEX_ID_TO_NAME, key)?.nth(0);
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

    /// Find Vertex by a specified integer id.
    pub fn find_vertex_name_by_id(&self, id: Id) -> Result<Option<Vertex>> {
        if !id.is_valid() {
            return Ok(None);
        }
        if id.is_virtual() {
            return Ok(self.virtual_map.lookup_vertex_name(id).cloned());
        }
        self.find_name_by_id(id)
            .map(|v| v.map(|n| Vertex(self.log.slice_to_bytes(n))))
    }

    /// Find the integer id matching the given name.
    pub fn find_id_by_name(&self, name: &[u8]) -> Result<Option<Id>> {
        for group in Group::ALL.iter() {
            if *group == Group::VIRTUAL {
                if let Some(found_id) = self.virtual_map.lookup_vertex_id(&Vertex::copy_from(name))
                {
                    return Ok(Some(found_id));
                }
            } else {
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
            } else {
                return bug(format!(
                    "new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                    id, name, existing_id, name
                ));
            }
        }

        if id.is_virtual() {
            self.virtual_map
                .insert_vertex_id_name(id, Vertex::copy_from(name));
            return Ok(());
        }

        let mut data = Vec::with_capacity(8 + Group::BYTES + name.len());
        data.extend_from_slice(&id.0.to_be_bytes());
        data.extend_from_slice(&id.group().bytes());
        data.extend_from_slice(name);
        self.log.append(data)?;
        self.map_version.bump();
        #[cfg(debug_assertions)]
        {
            let items = self.find_range(id, id).unwrap();
            assert_eq!(items[0], (id, name));
        }
        Ok(())
    }

    /// Find all (id, name) pairs in the `low..=high` range.
    fn find_range(&self, low: Id, high: Id) -> Result<Vec<(Id, &[u8])>> {
        let low_bytes = low.0.to_be_bytes();
        let high_bytes = high.0.to_be_bytes();
        let range = &low_bytes[..]..=&high_bytes[..];
        let mut items = Vec::new();
        if !low.is_virtual() {
            for entry in self.log.lookup_range(Self::INDEX_ID_TO_NAME, range)? {
                let (key, values) = entry?;
                let key: [u8; 8] = match key.as_ref().try_into() {
                    Ok(key) => key,
                    Err(_) => {
                        return bug("find_range got non-u64 keys in INDEX_ID_TO_NAME");
                    }
                };
                let id = Id(u64::from_be_bytes(key));
                for value in values {
                    let value = value?;
                    if value.len() < 8 {
                        return bug(format!(
                            "find_range got entry {:?} shorter than expected",
                            &value
                        ));
                    }
                    let name: &[u8] = &value[9..];
                    items.push((id, name));
                }
            }
        }
        if high.is_virtual() {
            for (id, name) in self.virtual_map.lookup_range(low, high) {
                items.push((*id, name.as_ref()))
            }
        }
        Ok(items)
    }

    fn remove_range(&mut self, low: Id, high: Id) -> Result<Vec<Vertex>> {
        let mut names = Vec::new();
        if !low.is_virtual() {
            // Step 1: Find (id, name) pairs in the range except for virtual items.
            debug_assert!(Group::ALL.contains(&(Group::VIRTUAL - 1)));
            let items = self.find_range(low, (Group::VIRTUAL - 1).max_id().min(high))?;
            names.extend(items.iter().map(|(_, bytes)| Vertex::copy_from(bytes)));
            // Step 2: Write a "delete" entry to delete those indexes.
            // The indexedlog index function (defined by log_open_options())
            // will handle it.
            let data = encode_deletion_entry(&items);
            self.log.append(data)?;
            // New map is not an "append-only" version of the previous map.
            // Re-create the VerLink to mark it as incompatible.
            self.map_version = VerLink::new();
        }
        // Step 3: Remove entries in the virtual_map.
        if high.is_virtual() {
            names.extend(self.virtual_map.remove_range(low, high)?);
        }
        Ok(names)
    }

    /// Lookup names by hex prefix.
    fn find_names_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<Vertex>> {
        let mut result = Vec::with_capacity(limit);
        for group in Group::ALL.iter().rev() {
            if result.len() >= limit {
                break;
            }
            if *group == Group::VIRTUAL {
                result.extend(self.virtual_map.lookup_vertexes_by_hex_prefix(
                    hex_prefix,
                    limit.saturating_sub(result.len()),
                )?);
            } else {
                let mut prefix = Vec::with_capacity(Group::BYTES * 2 + hex_prefix.len());
                prefix.extend_from_slice(&group.hex_bytes());
                prefix.extend_from_slice(hex_prefix);
                for entry in self
                    .log
                    .lookup_prefix_hex(Self::INDEX_GROUP_NAME_TO_ID, prefix)?
                {
                    if result.len() >= limit {
                        break;
                    }
                    let (k, _v) = entry?;
                    let vertex = Vertex(self.log.slice_to_bytes(&k[Group::BYTES..]));
                    if !result.contains(&vertex) {
                        result.push(vertex);
                    }
                }
            }
        }
        Ok(result)
    }
}

/// Encode a list of (id, name) pairs as an deletion entry.
/// The deletion entry will be consumed by the index functions defined by
/// `log_open_options()`.
fn encode_deletion_entry(items: &[(Id, &[u8])]) -> Vec<u8> {
    // Rough size for common 20-byte sha1 hashes.
    let len = IdMap::MAGIC_DELETION_PREFIX.len() + 9 + items.len() * 30;
    let mut data = Vec::with_capacity(len);
    data.extend_from_slice(IdMap::MAGIC_DELETION_PREFIX);
    data.write_vlq(items.len()).unwrap();
    for (id, name) in items {
        data.write_vlq(id.0).unwrap();
        data.write_vlq(name.len()).unwrap();
        data.extend_from_slice(name);
    }
    data
}

/// Decode `encode_deletion_entry` result.
/// Used by index functions in `log_open_options()`.
fn decode_deletion_entry(data: &[u8]) -> Result<Vec<(Id, &[u8])>> {
    assert!(data.starts_with(IdMap::MAGIC_DELETION_PREFIX));
    let mut data = &data[IdMap::MAGIC_DELETION_PREFIX.len()..];
    let n = data.read_vlq()?;
    let mut items = Vec::with_capacity(n);
    for _ in 0..n {
        let id: u64 = data.read_vlq()?;
        let id = Id(id);
        let name_len: usize = data.read_vlq()?;
        if name_len > data.len() {
            return bug("decode_deletion_id_names got incomplete input");
        }
        let (name, rest) = data.split_at(name_len);
        data = rest;
        items.push((id, name));
    }
    Ok(items)
}

impl fmt::Debug for IdMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IdMap {{\n")?;
        let vec = self.find_range(Id::MIN, Id::MAX).map_err(|_| fmt::Error)?;
        for (id, name) in vec {
            let name = if name.len() >= 20 {
                Vertex::copy_from(name).to_hex()
            } else {
                String::from_utf8_lossy(name).to_string()
            };
            write!(f, "  {}: {},\n", name, id)?;
        }
        write!(f, "}}\n")?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl IdConvert for IdMap {
    async fn vertex_id(&self, name: Vertex) -> Result<Id> {
        self.find_id_by_name(name.as_ref())?
            .ok_or_else(|| name.not_found_error())
    }
    async fn vertex_id_with_max_group(
        &self,
        name: &Vertex,
        max_group: Group,
    ) -> Result<Option<Id>> {
        self.find_id_by_name_with_max_group(name.as_ref(), max_group)
    }
    async fn vertex_name(&self, id: Id) -> Result<Vertex> {
        self.find_vertex_name_by_id(id)?
            .ok_or_else(|| id.not_found_error())
    }
    async fn contains_vertex_name(&self, name: &Vertex) -> Result<bool> {
        Ok(self.find_id_by_name(name.as_ref())?.is_some())
    }
    async fn contains_vertex_id_locally(&self, ids: &[Id]) -> Result<Vec<bool>> {
        let mut list = Vec::with_capacity(ids.len());
        for &id in ids {
            list.push(self.find_name_by_id(id)?.is_some());
        }
        Ok(list)
    }
    async fn contains_vertex_name_locally(&self, names: &[Vertex]) -> Result<Vec<bool>> {
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
    async fn remove_range(&mut self, low: Id, high: Id) -> Result<Vec<Vertex>> {
        IdMap::remove_range(self, low, high)
    }
}

impl Persist for IdMap {
    type Lock = File;

    fn lock(&mut self) -> Result<Self::Lock> {
        if self.log.iter_dirty().next().is_some() {
            return programming("lock() must be called without dirty in-memory entries");
        }
        let lock_file = {
            let path = self.path.join("wlock");
            // Some NFS implementation reports `EBADF` for `flock()` unless the file is opened
            // with `O_RDWR`.
            fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(&path)?
        };
        fs2::FileExt::lock_exclusive(&lock_file)?;
        Ok(lock_file)
    }

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.sync()?;
        self.map_version
            .associate_storage_version(self.map_id.clone(), self.log.version());
        Ok(())
    }
}

#[async_trait::async_trait]
impl PrefixLookup for IdMap {
    async fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<Vertex>> {
        self.find_names_by_hex_prefix(hex_prefix, limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode_deletion_entry() {
        let items: &[(Id, &[u8])] = &[
            (Id(0), b"a"),
            (Id(1), b"bb"),
            (Id(10), b"ccc"),
            (Id(20), b"dd"),
        ];
        let data = encode_deletion_entry(items);
        let decoded = decode_deletion_entry(&data).unwrap();
        assert_eq!(&decoded, items);
    }
}
