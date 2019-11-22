/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::fs::File;
use std::io;
use std::path::Path;
use std::result;
use std::sync::Arc;

use crate::errors::*;
use bytes::Bytes;
use memmap::Mmap;
use nom::IResult;

pub use mercurial_types::bdiff::{self, Delta};
pub use mercurial_types::{delta, HgBlob, HgBlobNode, HgEntryId, HgNodeHash, HgParents};

// Submodules
mod lz4;
mod parser;
mod revidx;

#[cfg(test)]
mod test;

pub use self::parser::Entry;
use self::parser::{Header, IdxFlags, Version};
pub use self::revidx::RevIdx;

#[derive(Debug)]
enum Datafile {
    Loaded(Vec<u8>),
    Mmap(Mmap),
}

impl Datafile {
    fn map<P: AsRef<Path>>(path: P) -> io::Result<Datafile> {
        let file = File::open(path)?;
        unsafe { Mmap::map(&file).map(Datafile::Mmap) }
    }

    fn as_slice(&self) -> &[u8] {
        match self {
            &Datafile::Loaded(ref data) => data.as_ref(),
            &Datafile::Mmap(ref mmap) => mmap.as_ref(),
        }
    }
}

impl AsRef<[u8]> for Datafile {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

fn map_io<T, F, R, E>(v: Option<T>, f: &mut F) -> result::Result<Option<R>, E>
where
    F: FnMut(T) -> result::Result<R, E>,
{
    match v {
        None => Ok(None),
        Some(v) => f(v).map(Option::Some),
    }
}

/// `Revlog` represents a Mercurial revlog structure
///
/// A Mercurial revlog logicically consists of two parts: an index containing metadata about each
/// revision in the file, and data about each one. These may be stored in one or two files,
/// depending on whether the data is inlined into the index or not.
///
/// This type represents the logical revlog. It allows iteration over the entries, fetching
/// entries at random, and extracting the data for each entry.
#[derive(Debug, Clone)]
pub struct Revlog {
    inner: Arc<RevlogInner>,
}

#[derive(Debug)]
struct RevlogInner {
    header: Header,
    idx: Datafile,
    data: Option<Datafile>,
    idxoff: BTreeMap<RevIdx, usize>,      // cache of index -> offset
    nodeidx: HashMap<HgNodeHash, RevIdx>, // cache of nodeid -> index
}

impl PartialEq<Self> for Revlog {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
impl Eq for Revlog {}

impl Revlog {
    fn init(idx: Datafile, data: Option<Datafile>) -> Result<Self> {
        let hdr = match parser::header(idx.as_slice()) {
            IResult::Done(_, hdr) => hdr,
            err => return Err(ErrorKind::Revlog(format!("Header parse failed: {:?}", err)).into()),
        };

        let mut data = data;
        if hdr.features.contains(parser::Features::INLINE) {
            data = None
        }

        let mut idxoff = BTreeMap::new();
        let mut nodeidx = HashMap::new();

        let mut inner = RevlogInner {
            header: hdr,
            idx,
            data,
            idxoff: BTreeMap::new(),
            nodeidx: HashMap::new(),
        };

        let mut off = 0;
        let mut i = RevIdx::zero();
        loop {
            let ent = inner.parse_entry(off);
            if let Ok(entry) = ent {
                idxoff.insert(i, off);
                nodeidx.insert(entry.nodeid, i);
                i = i.succ();
                off += inner.entry_size(Some(&entry));
            } else {
                break;
            }
        }
        inner.idxoff = idxoff;
        inner.nodeidx = nodeidx;
        Ok(Revlog {
            inner: Arc::new(inner),
        })
    }

    /// Construct a `Revlog` using in-memory data. The index is required; the data
    /// may not be if either its inlined into the data, or not required for operations.
    pub fn new(idx: Vec<u8>, data: Option<Vec<u8>>) -> Result<Self> {
        Self::init(Datafile::Loaded(idx), data.map(Datafile::Loaded))
    }

    /// Construct a `Revlog` from an index file at the given path. Data may be inlined
    /// not not required.
    pub fn from_idx_no_data<IP>(idxpath: IP) -> Result<Revlog>
    where
        IP: AsRef<Path>,
    {
        let idx = Datafile::map(idxpath).context("Can't map idxpath")?;

        let revlog = Revlog::init(idx, None)?;

        Ok(revlog)
    }

    /// Construct a `Revlog` from an index file and data file. If `datapath` is not provided
    /// (`None`), and the index file is not inlined, then it will replace the index file's
    /// extension with `.d` and attempt to open that. The operation will fail if that file can't
    /// be opened.
    pub fn from_idx_with_data<IP, DP>(idxpath: IP, datapath: Option<DP>) -> Result<Revlog>
    where
        IP: AsRef<Path> + Debug,
        DP: AsRef<Path> + Debug,
    {
        let mut revlog = Self::from_idx_no_data(&idxpath)
            .with_context(|| format!("Can't open index {:?}", idxpath))?;
        let datapath = datapath.as_ref().map(DP::as_ref);
        let idxpath = idxpath.as_ref();

        if !revlog.inner.have_data() {
            let datafile = match datapath {
                None => {
                    let path = idxpath.with_extension("d");
                    Datafile::map(&path)
                        .with_context(|| format!("Can't open data file {:?}", path))?
                }
                Some(path) => Datafile::map(&path)
                    .with_context(|| format!("Can't open data file {:?}", path))?,
            };
            Arc::get_mut(&mut revlog.inner).unwrap().data = Some(datafile);
        }

        Ok(revlog)
    }

    /// Return `true` if the `Revlog` has the data it requires - ie, the data is either inlined,
    /// or a data file has been provided.
    pub fn have_data(&self) -> bool {
        self.inner.have_data()
    }

    /// Get the `Revlog`s header.
    pub fn get_header(&self) -> Header {
        self.inner.header
    }

    /// Return an `Entry` entry from the `RevIdx`.
    pub fn get_entry(&self, idx: RevIdx) -> Result<Entry> {
        self.inner.get_entry(idx)
    }

    /// Return an `Entry` entry from the `RevIdx`.
    pub fn is_ext_by_nodeid(&self, nodeid: HgNodeHash) -> Result<bool> {
        self.inner.is_ext_by_nodeid(nodeid)
    }

    /// Return the ordinal index of an entry with the given nodeid.
    pub fn get_idx_by_nodeid(&self, nodeid: HgNodeHash) -> Result<RevIdx> {
        self.inner.get_idx_by_nodeid(nodeid)
    }

    /// Return the ordinal index of an entry with the given nodeid.
    pub fn get_entry_by_id(&self, entryid: HgEntryId) -> Result<Entry> {
        let nodeid = entryid.clone().into_nodehash();
        self.inner.get_entry_by_nodeid(nodeid)
    }

    /// Return a `Chunk` for a revision at `RevIdx`.
    ///
    /// A `Chunk` either represents the literal
    /// text of the change, or a series of deltas against a previous version; the exact
    /// mechanism of applying the deltas depends on whether the `RevLog` has the `GENERAL_DELTA`
    /// flag set or not.
    pub fn get_chunk(&self, idx: RevIdx) -> Result<Chunk> {
        self.inner.get_chunk(idx)
    }

    pub fn get_rev(&self, tgtidx: RevIdx) -> Result<HgBlobNode> {
        self.inner.get_rev(tgtidx)
    }

    pub fn get_rev_by_nodeid(&self, id: HgNodeHash) -> Result<HgBlobNode> {
        self.inner.get_rev_by_nodeid(id)
    }

    pub fn get_rev_parents_by_nodeid(&self, id: HgNodeHash) -> Result<HgParents> {
        self.inner.get_rev_parents_by_nodeid(id)
    }

    /// Return the set of head revisions in a revlog
    pub fn get_heads(&self) -> Result<HashSet<HgNodeHash>> {
        self.inner.get_heads()
    }
}

impl RevlogInner {
    // Parse an entry at an offset, doing the correction for the overlap of the first
    // entry and the header.
    fn parse_entry(&self, off: usize) -> Result<Entry> {
        let res = match self.header.version {
            Version::Revlog0 => parser::index0(&self.idx.as_slice()[off..]),
            Version::RevlogNG => parser::indexng(&self.idx.as_slice()[off..]),
        };

        match res {
            IResult::Done(_, mut res) => {
                if off == 0 {
                    res.offset &= 0xffff;
                }
                Ok(res)
            }
            err => {
                return Err(ErrorKind::Revlog(format!(
                    "failed to parse entry offset {}: {:?}",
                    off, err
                ))
                .into());
            }
        }
    }

    fn fixed_entry_size(&self) -> usize {
        match self.header.version {
            Version::Revlog0 => parser::index0_size(),
            Version::RevlogNG => parser::indexng_size(),
        }
    }

    fn entry_size(&self, ent: Option<&Entry>) -> usize {
        let mut sz = self.fixed_entry_size();
        if self.header.features.contains(parser::Features::INLINE) {
            sz += ent.expect("inline needs ent").compressed_len as usize;
        }
        sz
    }

    fn offset_for_idx(&self, idx: RevIdx) -> Option<usize> {
        if self.header.features.contains(parser::Features::INLINE) {
            self.idxoff.get(&idx).cloned()
        } else {
            Some(idx * self.entry_size(None) as usize)
        }
    }

    fn have_data(&self) -> bool {
        // inline implies no data
        assert!(!self.header.features.contains(parser::Features::INLINE) || self.data.is_none());

        // have data if inline or data is non-None
        self.header.features.contains(parser::Features::INLINE) || self.data.is_some()
    }

    /// Return an `Entry` entry from the `RevIdx`.
    fn get_entry(&self, idx: RevIdx) -> Result<Entry> {
        if let Some(off) = self.offset_for_idx(idx) {
            // cache hit or computed
            self.parse_entry(off)
        } else {
            Err(ErrorKind::Revlog(format!("rev {:?} not found", idx)).into())
        }
    }

    // Return an `Entry` entry from the `RevIdx`.
    fn is_ext_by_nodeid(&self, nodeid: HgNodeHash) -> Result<bool> {
        self.get_entry_by_nodeid(nodeid)
            .map(|entry| entry.flags.contains(IdxFlags::EXTSTORED))
    }

    /// Return the ordinal index of an entry with the given nodeid.
    fn get_idx_by_nodeid(&self, nodeid: HgNodeHash) -> Result<RevIdx> {
        match self.nodeidx.get(&nodeid).cloned() {
            Some(idx) => Ok(idx), // cache hit
            None => Err(ErrorKind::Revlog(format!("nodeid {} not found", nodeid)).into()),
        }
    }

    fn get_entry_by_nodeid(&self, nodeid: HgNodeHash) -> Result<Entry> {
        self.get_idx_by_nodeid(nodeid)
            .and_then(|idx| self.get_entry(idx))
    }

    /// Return a `Chunk` for a revision at `RevIdx`.
    ///
    /// A `Chunk` either represents the literal
    /// text of the change, or a series of deltas against a previous version; the exact
    /// mechanism of applying the deltas depends on whether the `RevLog` has the `GENERAL_DELTA`
    /// flag set or not.
    fn get_chunk(&self, idx: RevIdx) -> Result<Chunk> {
        if !self.have_data() {
            return Err(failure_ext::err_msg("Can't get chunks without data"));
        }

        let entry = self.get_entry(idx)?;

        let (chunkdata, start) = if self.header.features.contains(parser::Features::INLINE) {
            let off = self.offset_for_idx(idx).expect("not cached?");
            let start = off + self.fixed_entry_size();

            (self.idx.as_slice(), start)
        } else {
            let start = entry.offset as usize;

            (
                self.data
                    .as_ref()
                    .expect("non-inline has no data")
                    .as_slice(),
                start,
            )
        };
        let end = start + (entry.compressed_len as usize);
        let chunkdata = &chunkdata[start..end];

        // If the entry has baserev that is equal to it's idx, then the chunk is literal data.
        // Otherwise its 0 or more deltas against the baserev. If its general delta, then the
        // baserev itself might also be delta, otherwise its all the deltas from baserev..idx.
        if Some(idx) == entry.baserev {
            if chunkdata.is_empty() {
                Ok(Chunk::Literal(vec![]))
            } else {
                match parser::literal(chunkdata) {
                    IResult::Done(rest, _) if rest.len() != 0 => Err(ErrorKind::Revlog(format!(
                        "Failed to unpack literal: {} remains, {:?}",
                        rest.len(),
                        &rest[..16]
                    ))
                    .into()),
                    IResult::Done(_, literal) => Ok(Chunk::Literal(literal)),
                    err => Err(
                        ErrorKind::Revlog(format!("Failed to unpack literal: {:?}", err)).into(),
                    ),
                }
            }
        } else {
            match parser::deltachunk(chunkdata) {
                IResult::Done(rest, _) if rest.len() != 0 => Err(ErrorKind::Revlog(format!(
                    "Failed to unpack details: {} remains, {:?}",
                    rest.len(),
                    &rest[..16]
                ))
                .into()),
                IResult::Done(_, deltas) => Ok(Chunk::Deltas(deltas)),
                err => Err(ErrorKind::Revlog(format!("Failed to unpack deltas: {:?}", err)).into()),
            }
        }
    }

    fn is_general_delta(&self) -> bool {
        self.header
            .features
            .contains(parser::Features::GENERAL_DELTA)
    }

    fn construct_simple(&self, tgtidx: RevIdx) -> Result<Vec<u8>> {
        assert!(!self.is_general_delta());

        let entry = self.get_entry(tgtidx)?;

        // if there's no baserev, then use the target as a baserev (it should be literal)
        let baserev = entry.baserev.map(Into::into).unwrap_or(tgtidx);

        // XXX: Fix this to use delta::Delta instead of bdiff::Delta.

        // non-general delta - baserev should be literal, then we applying
        // each delta up to idx
        let mut data = Vec::new();
        let mut chain = Vec::new();
        for idx in baserev.range_to(tgtidx.succ()) {
            let chunk = self
                .get_chunk(idx)
                .with_context(|| format_err!("simple tgtidx {:?} idx {:?}", tgtidx, idx));

            match chunk? {
                Chunk::Literal(v) => {
                    data = v;
                    chain.clear();
                }
                Chunk::Deltas(deltas) => {
                    chain.push(deltas);
                }
            }
        }

        delta::compat::apply_deltas(data.as_ref(), chain)
    }

    fn construct_general(&self, tgtidx: RevIdx) -> Result<Vec<u8>> {
        assert!(self.is_general_delta());

        let mut chunks = Vec::new();
        let mut idx = tgtidx;

        // general delta - walk backwards until we hit a literal, collecting chunks on the way
        let data = loop {
            chunks.push(idx);

            let chunk = self.get_chunk(idx).with_context(|| {
                format_err!("construct_general tgtidx {:?} idx {:?}", tgtidx, idx)
            })?;

            // We have three valid cases:
            // 1) Literal chunk - this is possible only if baserev == idx
            // 2) Delta against empty string - this is possible only if baserev is None.
            //    In core hg it matches a case where baserev == -1.
            // 3) Delta against non-empty string. Only if baserev is Some(...) and baserev < idx.
            match self.get_entry(idx)?.baserev {
                Some(baseidx) if idx == baseidx => {
                    // This is a literal
                    match chunk {
                        Chunk::Literal(v) => {
                            chunks.pop();
                            break v;
                        }
                        _ => {
                            Err(ErrorKind::Revlog(format!("expected a literal")))?;
                        }
                    }
                }
                Some(baseidx) if idx > baseidx => {
                    idx = baseidx;
                }
                Some(baseidx) => {
                    Err(ErrorKind::Revlog(format!(
                        "baserev {:?} >= idx {:?}",
                        baseidx, idx
                    )))?;
                }
                None => match chunk {
                    // This is a delta against "-1" revision i.e. empty revision
                    Chunk::Deltas(_) => {
                        break vec![];
                    }
                    _ => {
                        Err(ErrorKind::Revlog(format!(
                            "expected a delta against empty string"
                        )))?;
                    }
                },
            }
        };

        // XXX: Fix this to use delta::Delta instead of bdiff::Delta.
        let chain = chunks.into_iter().rev().map(|idx| {
            let chunk = self.get_chunk(idx);

            match chunk {
                Ok(Chunk::Deltas(deltas)) => deltas,
                _ => panic!("Literal text found in delta chain."),
            }
        });

        delta::compat::apply_deltas(data.as_ref(), chain)
    }

    fn parse_parents(&self, entry: &Entry) -> Result<(Option<HgNodeHash>, Option<HgNodeHash>)> {
        let mut pnodeid = |p| {
            let pn = self.get_entry(p);
            pn.map(|n| n.nodeid)
        };
        let p1 = map_io(entry.p1, &mut pnodeid)?;
        let p2 = map_io(entry.p2, &mut pnodeid)?;
        Ok((p1, p2))
    }

    fn make_node(&self, entry: &Entry, blob: HgBlob) -> Result<HgBlobNode> {
        let (p1, p2) = self.parse_parents(entry)?;

        Ok(HgBlobNode::new(blob, p1, p2))
    }

    fn get_rev(&self, tgtidx: RevIdx) -> Result<HgBlobNode> {
        if !self.have_data() {
            return Err(failure_ext::err_msg("Need data to assemble revision"));
        }

        let entry = self.get_entry(tgtidx)?;

        let data = if self.is_general_delta() {
            self.construct_general(tgtidx)?
        } else {
            self.construct_simple(tgtidx)?
        };

        self.make_node(&entry, HgBlob::from(Bytes::from(data)))
    }

    fn get_parents(&self, tgtidx: RevIdx) -> Result<HgParents> {
        let entry = self.get_entry(tgtidx)?;
        let (p1, p2) = self.parse_parents(&entry)?;

        Ok(HgParents::new(p1, p2))
    }

    fn get_rev_by_nodeid(&self, id: HgNodeHash) -> Result<HgBlobNode> {
        self.get_idx_by_nodeid(id).and_then(move |idx| {
            self.get_rev(idx)
                .with_context(|| format!("can't get rev for id {}", id))
        })
    }

    fn get_rev_parents_by_nodeid(&self, id: HgNodeHash) -> Result<HgParents> {
        self.get_idx_by_nodeid(id).and_then(move |idx| {
            self.get_parents(idx)
                .with_context(|| format!("can't get parents for id {}", id))
        })
    }

    /// Return the set of head revisions in a revlog
    fn get_heads(&self) -> Result<HashSet<HgNodeHash>> {
        // Current set of candidate heads
        let mut heads = HashMap::new();

        for (idx, entry) in self.into_iter() {
            // New entry could be a head
            heads.insert(idx, entry);

            // This entry's parent(s) are non-heads
            if let Some(p1) = entry.p1 {
                let _ = heads.remove(&p1);
            }

            if let Some(p2) = entry.p2 {
                let _ = heads.remove(&p2);
            }
        }

        // Convert to a set of nodeids
        Ok(heads.values().map(|n| n.nodeid).collect())
    }
}

/// Data associated with a revision.
///
/// XXX internal detail?
#[derive(Debug)]
pub enum Chunk {
    /// Literal text of the revision
    Literal(Vec<u8>),
    /// Vector of `Delta`s against a previous version
    Deltas(Vec<Delta>),
}

struct RevlogInnerIter<'a>(&'a RevlogInner, RevIdx);

impl<'a> IntoIterator for &'a RevlogInner {
    type Item = <Self::IntoIter as Iterator>::Item;
    type IntoIter = RevlogInnerIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        RevlogInnerIter(self, RevIdx::zero())
    }
}

impl<'a> Iterator for RevlogInnerIter<'a> {
    type Item = (RevIdx, Entry);

    fn next(&mut self) -> Option<Self::Item> {
        let idx = self.1;
        let ret = self.0.get_entry(idx).ok();
        self.1 = idx.succ();
        ret.map(|r| (idx, r))
    }
}

#[derive(Debug)]
pub struct RevlogIter(Arc<RevlogInner>, RevIdx);

impl IntoIterator for Revlog {
    type Item = <Self::IntoIter as Iterator>::Item;
    type IntoIter = RevlogIter;

    fn into_iter(self) -> Self::IntoIter {
        RevlogIter(self.inner.clone(), RevIdx::zero())
    }
}

impl<'a> IntoIterator for &'a Revlog {
    type Item = <Self::IntoIter as Iterator>::Item;
    type IntoIter = RevlogIter;

    fn into_iter(self) -> Self::IntoIter {
        RevlogIter(self.inner.clone(), RevIdx::zero())
    }
}

impl RevlogIter {
    pub fn seek(&mut self, idx: RevIdx) {
        self.1 = idx;
    }
}

impl Iterator for RevlogIter {
    type Item = (RevIdx, Entry);

    fn next(&mut self) -> Option<Self::Item> {
        let revlog = &self.0;

        let idx = self.1;
        let ret = revlog.get_entry(idx).ok();
        self.1 = idx.succ();
        ret.map(|r| (idx, r))
    }
}
