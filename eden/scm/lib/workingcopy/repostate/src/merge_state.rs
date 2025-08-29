/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use byteorder::WriteBytesExt;
use serde::Serialize;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;
use types::hgid::NULL_ID;

/// MergeState represents the repo state when merging two commits.
///
/// Basically, MergeState records which commits are being merged, and the state
/// of conflicting files (i.e. unresoled/resolved).
///
/// MergeState is serialized as a list of records. Each record contains an
/// arbitrary list of strings and an associated type. This `type` should be a
/// letter. If `type` is uppercase, the record is mandatory: versions of Sapling
/// that don't support it should abort. If `type` is lowercase, the record can
/// be safely ignored.
///
/// Currently known records:
///
/// L: the node of the "local" part of the merge (hexified version)
/// O: the node of the "other" part of the merge (hexified version)
/// F: a file to be merged entry
/// C: a change/delete or delete/change conflict
/// D: a file that the external merge driver will merge internally
///    (experimental)
/// P: a path conflict (file vs directory)
/// S: subtree merges
/// m: the external merge driver defined for this merge plus its run state
///    (experimental)
/// f: a (filename, dictionary) tuple of optional values for a given file
/// X: unsupported mandatory record type (used in tests)
/// x: unsupported advisory record type (used in tests)
/// l: the labels for the parts of the merge.
///
/// Merge driver run states (experimental), see [`MergeDriverState`] for the details.
///
/// Merge record states (stored in self._state, indexed by filename):
/// u: unresolved conflict
/// r: resolved conflict
/// pu: unresolved path conflict (file conflicts with directory)
/// pr: resolved path conflict
/// d: driver-resolved conflict
#[derive(Default)]
pub struct MergeState {
    // commits being merged
    local: Option<HgId>,
    other: Option<HgId>,

    subtree_merges: Vec<SubtreeMerge>,

    // contextual labels for local/other/base
    labels: Vec<String>,

    // conflicting files
    files: HashMap<RepoPathBuf, FileInfo>,

    // merge driver definition at start of merge so we can detect merge driver
    // config changing during merge.
    merge_driver: Option<(String, MergeDriverState)>,

    // list of unsupported record types and accompanying record data, if any
    unsupported_records: Vec<(String, Vec<String>)>,

    // allows writing arbitrary records for testing purposes
    raw_records: Vec<(u8, Vec<String>)>,
}

#[derive(Debug)]
pub struct UnsupportedMergeRecords(pub MergeState);

impl std::fmt::Display for UnsupportedMergeRecords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0.unsupported_records)
    }
}

impl MergeState {
    pub fn new(local: Option<HgId>, other: Option<HgId>, labels: Vec<String>) -> Self {
        Self {
            local,
            other,
            labels,
            ..Default::default()
        }
    }

    pub fn read(path: &Path) -> Result<Option<Self>> {
        match fs_err::File::open(path) {
            Ok(mut file) => Ok(Some(
                Self::deserialize(&mut file).context("deserializing merge state")?,
            )),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).context("opening merge state"),
        }
    }

    pub fn files(&self) -> &HashMap<RepoPathBuf, FileInfo> {
        &self.files
    }

    pub fn local(&self) -> Option<&HgId> {
        self.local.as_ref()
    }

    pub fn other(&self) -> Option<&HgId> {
        self.other.as_ref()
    }

    pub fn merge_driver(&self) -> Option<&(String, MergeDriverState)> {
        self.merge_driver.as_ref()
    }

    pub fn set_merge_driver(&mut self, md: Option<(String, MergeDriverState)>) {
        self.merge_driver = md;
    }

    pub fn unsupported_records(&self) -> &[(String, Vec<String>)] {
        &self.unsupported_records
    }

    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    pub fn add_subtree_merge(
        &mut self,
        from_commit: HgId,
        from_path: RepoPathBuf,
        to_path: RepoPathBuf,
        from_url: Option<String>,
    ) {
        let subtree_merge = SubtreeMerge {
            from_commit,
            from_path,
            to_path,
            from_url,
        };
        self.subtree_merges.push(subtree_merge);
    }

    pub fn subtree_merges(&self) -> &[SubtreeMerge] {
        &self.subtree_merges
    }

    pub fn insert(&mut self, path: RepoPathBuf, data: Vec<String>) -> Result<()> {
        if data.is_empty() {
            bail!("invalid empty merge data for {}", path);
        }

        self.files.insert(
            path,
            FileInfo {
                state: ConflictState::from_record(&data[0])?,
                data,
                extras: HashMap::new(),
            },
        );

        Ok(())
    }

    pub fn add_raw_record(&mut self, record_type: u8, data: Vec<String>) {
        self.raw_records.push((record_type, data));
    }

    pub fn remove(&mut self, path: &RepoPath) {
        self.files.remove(path);
    }

    pub fn set_extra(&mut self, path: &RepoPath, key: String, value: String) -> Result<()> {
        if let Some(info) = self.files.get_mut(path) {
            info.extras.insert(key, value);
            Ok(())
        } else {
            bail!("no such file {path} to set extra");
        }
    }

    pub fn set_state(&mut self, path: &RepoPath, state: String) -> Result<()> {
        if let Some(info) = self.files.get_mut(path) {
            if info.data.is_empty() {
                bail!("invalid empty merge data when setting state for {path}");
            }
            info.state = ConflictState::from_record(&state)?;
            info.data[0] = state;
            Ok(())
        } else {
            bail!("no such file {path} to set state");
        }
    }

    pub fn deserialize(data: &mut dyn Read) -> Result<Self> {
        let mut data = std::io::BufReader::new(data);

        let mut ms = Self::default();

        loop {
            let record_type = match data.read_u8() {
                Ok(t) => t,
                Err(err) if err.kind() == ErrorKind::UnexpectedEof => break,
                Err(err) => return Err(err).context("reading record type"),
            };

            let record_length = data
                .read_u32::<BigEndian>()
                .context("reading record length")?;

            let mut record_data = vec![0; record_length as usize];
            data.read_exact(&mut record_data)
                .context("reading record data")?;

            fn split_strings(data: Vec<u8>) -> Result<(String, Vec<String>)> {
                let mut fields = data.split(|b| *b == 0);
                let first = fields.next().context("first string field")?;
                Ok((
                    std::str::from_utf8(first)?.to_owned(),
                    fields
                        .map(|d| Ok(std::str::from_utf8(d)?.to_owned()))
                        .collect::<Result<_>>()
                        .context("reading record strings")?,
                ))
            }

            match record_type {
                b'L' => {
                    ms.local = Some(HgId::from_hex(&record_data).context("parsing local node")?);
                }
                b'O' => {
                    ms.other = Some(HgId::from_hex(&record_data).context("parsing other node")?);
                }
                b'm' => {
                    let (first, rest) = split_strings(record_data)?;
                    ms.merge_driver = Some((
                        first,
                        rest.first().map_or(MergeDriverState::Unmarked, |s| {
                            MergeDriverState::from_py_string(s.as_str())
                        }),
                    ));
                }
                b'F' | b'D' | b'C' | b'P' => {
                    let (first, rest) = split_strings(record_data)?;
                    ms.files.insert(
                        first.try_into().context("file path")?,
                        FileInfo {
                            state: ConflictState::from_record(
                                rest.first().context("record state")?,
                            )?,
                            data: rest,
                            extras: HashMap::new(),
                        },
                    );
                }
                b'S' => {
                    let (first, rest) = split_strings(record_data)?;
                    let from_commit =
                        HgId::from_hex(first.as_bytes()).context("subtree merge from-commit")?;

                    let (from_path, to_path, from_url) = match rest.as_slice() {
                        [from_path, to_path] => (from_path.clone(), to_path.clone(), None),
                        [from_path, to_path, from_url] => {
                            (from_path.clone(), to_path.clone(), Some(from_url.clone()))
                        }
                        _ => {
                            bail!(
                                "subtree merge should have two paths and an optional from-url: {} {:?}",
                                first,
                                rest
                            );
                        }
                    };

                    let from_path = from_path.try_into().context("subtree from-path")?;
                    let to_path = to_path.try_into().context("subtree to-path")?;
                    let subtree_merge = SubtreeMerge {
                        from_commit,
                        from_path,
                        to_path,
                        from_url,
                    };
                    ms.subtree_merges.push(subtree_merge);
                }
                b'f' => {
                    let (first, mut rest) = split_strings(record_data)?;

                    if rest.len() % 2 != 0 {
                        bail!("odd number of extras for {}: {:?}", first, rest);
                    }

                    let path: RepoPathBuf = first.try_into().context("extra file path")?;

                    // We assume file record comes before extras record.
                    if let Some(file) = ms.files.get_mut(&path) {
                        while let (Some(value), Some(key)) = (rest.pop(), rest.pop()) {
                            file.extras.insert(key, value);
                        }
                    }
                }
                b'l' => {
                    let (first, rest) = split_strings(record_data)?;

                    ms.labels = std::iter::once(first)
                        .chain(rest)
                        .filter(|l| !l.is_empty())
                        .collect();
                }
                _ => {
                    let (first, rest) = split_strings(record_data).unwrap_or_default();
                    ms.unsupported_records.push((
                        util::utf8::escape_non_utf8(&[record_type]),
                        std::iter::once(first).chain(rest).collect(),
                    ))
                }
            };
        }

        // Upper case record types are required. Lower case are optional.
        if ms
            .unsupported_records
            .iter()
            .any(|(t, _)| t.len() != 1 || !t.as_bytes()[0].is_ascii_lowercase())
        {
            return Err(anyhow!("unsupported merge record types"))
                .context(UnsupportedMergeRecords(ms));
        }

        Ok(ms)
    }

    pub fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        let w = &mut std::io::BufWriter::new(w);

        fn write_record(
            w: &mut dyn Write,
            record_type: u8,
            first: &str,
            rest: &[impl AsRef<str>],
        ) -> Result<()> {
            w.write_u8(record_type)?;
            w.write_u32::<BigEndian>(
                (first.len() + rest.iter().fold(0, |a, v| a + v.as_ref().len()) + rest.len())
                    as u32,
            )?;

            w.write_all(first.as_bytes())?;

            for data in rest.iter() {
                w.write_u8(0)?;
                w.write_all(data.as_ref().as_bytes())?;
            }

            Ok(())
        }

        if let Some(local) = &self.local {
            write_record(w, b'L', &local.to_hex(), &Vec::<&str>::new())?;
        }

        if let Some(other) = &self.other {
            write_record(w, b'O', &other.to_hex(), &Vec::<&str>::new())?;
        }

        for subtree_merge in &self.subtree_merges {
            let commit = &subtree_merge.from_commit().to_hex();
            let mut rest = vec![
                subtree_merge.from_path().as_str(),
                subtree_merge.to_path().as_str(),
            ];
            if let Some(url) = subtree_merge.from_url() {
                rest.push(url);
            }
            write_record(w, b'S', commit, &rest)?;
        }

        if let Some((md, mds)) = &self.merge_driver {
            write_record(w, b'm', md, &[mds.to_py_string()])?;
        }

        for (path, info) in self.files.iter() {
            write_record(w, info.record_type(), path.as_str(), &info.data)?;

            if !info.extras.is_empty() {
                write_record(
                    w,
                    b'f',
                    path.as_str(),
                    &info
                        .extras
                        .iter()
                        .map(|(k, v)| format!("{k}\x00{v}"))
                        .collect::<Vec<_>>(),
                )?;
            }
        }

        if !self.labels.is_empty() {
            write_record(w, b'l', &self.labels[0], &self.labels[1..])?;
        }

        for (rt, data) in &self.raw_records {
            write_record(w, *rt, &data[0], &data[1..])?;
        }

        // Flush explicitly to propagate errors.
        w.flush()?;

        Ok(())
    }

    pub fn is_unresolved(&self) -> bool {
        self.files
            .iter()
            .any(|(_, info)| info.state.is_unresolved())
            || self
                .merge_driver
                .as_ref()
                .is_some_and(|(_, state)| *state != MergeDriverState::Success)
    }
}

impl std::fmt::Debug for MergeState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(local) = &self.local {
            writeln!(f, "local: {local}")?;
        }

        if let Some(other) = &self.other {
            writeln!(f, "other: {other}")?;
        }

        if let Some((md, mds)) = &self.merge_driver {
            writeln!(
                f,
                r#"merge driver: {} (state "{}")"#,
                md,
                mds.to_py_string()
            )?;
        }

        if !self.subtree_merges.is_empty() {
            writeln!(f, "subtree merges:")?;
            for subtree_merge in &self.subtree_merges {
                let from_commit = subtree_merge.from_commit();
                let from_path = subtree_merge.from_path();
                let to_path = subtree_merge.to_path();
                let from_url = subtree_merge.from_url();
                match from_url {
                    None => {
                        writeln!(
                            f,
                            "  from_commit: {}, from: {}, to: {}",
                            from_commit, from_path, to_path
                        )?;
                    }
                    Some(url) => {
                        writeln!(
                            f,
                            "  from_commit: {}, from: {}, to: {}, url: {}",
                            from_commit, from_path, to_path, url
                        )?;
                    }
                }
            }
        }

        if !self.labels.is_empty() {
            writeln!(f, "labels:")?;
            for (t, n) in ["local", "other", "base"].iter().zip(&self.labels) {
                writeln!(f, "  {t}: {n}")?;
            }
        }

        fn hash_or_null(h: &str) -> &str {
            if h == NULL_ID.to_hex() { "null" } else { h }
        }

        let mut paths: Vec<_> = self.files.keys().collect();
        paths.sort_by_key(|p| p.as_str());
        for p in paths {
            let file = self.files.get(p).unwrap();
            let record_type = util::utf8::escape_non_utf8(&[file.record_type()]);

            if record_type == "P" {
                if file.data.len() != 3 {
                    writeln!(
                        f,
                        r#"file: {} (record type "{}", unexpected data: {:?})"#,
                        p, record_type, file.data,
                    )?;
                } else {
                    writeln!(
                        f,
                        r#"file: {} (record type "{}", state "{}", renamed to {}, origin "{}")"#,
                        p, record_type, file.data[0], file.data[1], file.data[2],
                    )?;
                }
            } else if file.data.len() != 8 {
                writeln!(
                    f,
                    r#"file: {} (record type "{}", unexpected data: {:?})"#,
                    p, record_type, file.data,
                )?;
            } else {
                writeln!(
                    f,
                    r#"file: {} (record type "{}", state "{}", hash {})"#,
                    p,
                    record_type,
                    file.data[0],
                    hash_or_null(&file.data[1]),
                )?;

                writeln!(
                    f,
                    r#"  local path: {} (flags "{}")"#,
                    file.data[2], file.data[7],
                )?;

                writeln!(
                    f,
                    "  ancestor path: {} (node {})",
                    file.data[3],
                    hash_or_null(&file.data[4]),
                )?;

                writeln!(
                    f,
                    "  other path: {} (node {})",
                    file.data[5],
                    hash_or_null(&file.data[6]),
                )?;

                if !file.extras.is_empty() {
                    writeln!(
                        f,
                        "  extras: {}",
                        file.extras
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    )?;
                }
            }
        }

        for (t, d) in &self.unsupported_records {
            writeln!(f, r#"unsupported record "{}" (data {:?})"#, t, d)?;
        }

        Ok(())
    }
}

/// SubtreeMerge stores metadata for subtree merge operations: subtree graft and subtree merge
#[derive(Debug, PartialEq, Clone, Serialize)]
pub struct SubtreeMerge {
    from_commit: HgId,
    from_path: RepoPathBuf,
    to_path: RepoPathBuf,
    // The external repo URL (present only for cross-repo merges)
    from_url: Option<String>,
}

impl SubtreeMerge {
    pub fn from_commit(&self) -> &HgId {
        &self.from_commit
    }

    pub fn from_path(&self) -> &RepoPath {
        &self.from_path
    }

    pub fn to_path(&self) -> &RepoPath {
        &self.to_path
    }

    pub fn from_url(&self) -> Option<&str> {
        self.from_url.as_deref()
    }
}

#[derive(Debug)]
pub struct FileInfo {
    // arbitrary key->value data (seems to only be used for "ancestorlinknode")
    extras: HashMap<String, String>,
    state: ConflictState,

    // An opaque-to-Rust tuple of data.
    //
    // For path conflicts it contains:
    //
    //    [
    //      <merge state code>,
    //      <renamed name>,
    //      l(ocal) | r(emote),
    //    ]
    // For other conflicts it contains:
    //
    //    [
    //      <merge state code>,
    //      <hash of "local" file path>,
    //      <local file path>,
    //      <ancestor file path>,
    //      <ancestor file node hex>,
    //      <other file path>,
    //      <other file node hex>,
    //      <local file flags>,
    //    ]
    data: Vec<String>,
}

impl FileInfo {
    pub fn extras(&self) -> &HashMap<String, String> {
        &self.extras
    }

    pub fn data(&self) -> &Vec<String> {
        &self.data
    }

    pub fn record_type(&self) -> u8 {
        match self.state {
            ConflictState::Unresolved | ConflictState::Resolved => {
                if self.data.get(1).is_some_and(|h| *h == NULL_ID.to_hex())
                    || self.data.get(6).is_some_and(|h| *h == NULL_ID.to_hex())
                {
                    // Infer 'C'hange/delete conflict if one of the file nodes is null.
                    b'C'
                } else {
                    // Normal conflicts are stored in "F" records.
                    b'F'
                }
            }
            // Path conflicts are stored in "P" records.
            ConflictState::UnresolvedPath | ConflictState::ResolvedPath => b'P',
            // Driver resolved are stored in "D" records.
            ConflictState::DriverResolved => b'D',
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConflictState {
    Unresolved,
    Resolved,
    UnresolvedPath,
    ResolvedPath,
    DriverResolved,
}

impl ConflictState {
    fn from_record(name: &str) -> Result<Self> {
        Ok(match name {
            "d" => Self::DriverResolved,
            "pu" => Self::UnresolvedPath,
            "pr" => Self::ResolvedPath,
            "u" => Self::Unresolved,
            "r" => Self::Resolved,
            _ => bail!("unknown merge record state '{}'", name),
        })
    }

    fn is_unresolved(&self) -> bool {
        match self {
            // DriverResolved means "will be resolved by driver", not "driver already resolved".
            Self::Unresolved | Self::UnresolvedPath | Self::DriverResolved => true,
            _ => false,
        }
    }
}

/// Merge driver run states:
/// u: driver-resolved files unmarked -- needs to be run next time we're about
///    to resolve or commit
/// m: driver-resolved files marked -- only needs to be run before commit
/// s: success/skipped -- does not need to be run any more
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MergeDriverState {
    Unmarked,
    Marked,
    Success,
}

impl MergeDriverState {
    pub fn from_py_string(s: &str) -> Self {
        match s {
            "m" => MergeDriverState::Marked,
            "s" => MergeDriverState::Success,
            // When in doubt, re-run drivers (they should be idempotent).
            _ => MergeDriverState::Unmarked,
        }
    }

    pub fn to_py_string(&self) -> &'static str {
        match self {
            MergeDriverState::Unmarked => "u",
            MergeDriverState::Marked => "m",
            MergeDriverState::Success => "s",
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_is_unresolved() -> Result<()> {
        let mut ms = MergeState::default();
        assert!(!ms.is_unresolved());

        ms.insert("foo".to_string().try_into()?, vec!["u".to_string()])?;
        assert!(ms.is_unresolved());

        ms.set_state("foo".try_into()?, "pu".to_string())?;
        assert!(ms.is_unresolved());

        ms.set_state("foo".try_into()?, "d".to_string())?;
        assert!(ms.is_unresolved());

        ms.set_state("foo".try_into()?, "r".to_string())?;
        assert!(!ms.is_unresolved());

        ms.set_merge_driver(Some(("my driver".to_string(), MergeDriverState::Marked)));
        assert!(ms.is_unresolved());

        Ok(())
    }

    #[test]
    fn test_subtree_merges_without_url() -> Result<()> {
        let mut ms = MergeState::default();
        assert!(ms.subtree_merges().is_empty());

        let from_path = RepoPathBuf::from_string("foo/a".to_string())?;
        let to_path = RepoPathBuf::from_string("foo/b".to_string())?;
        let from_commit = HgId::from_byte_array([0x11; HgId::len()]);
        let from_url = None;

        let subtree_merge = SubtreeMerge {
            from_commit: from_commit.clone(),
            from_path: from_path.clone(),
            to_path: to_path.clone(),
            from_url: from_url.clone(),
        };

        ms.add_subtree_merge(from_commit, from_path.clone(), to_path.clone(), from_url);
        assert_eq!(ms.subtree_merges(), &[subtree_merge.clone()]);

        let mut data = Vec::new();
        ms.serialize(&mut data)?;

        let mut buffer = &data[..];
        let ms2 = MergeState::deserialize(&mut buffer)?;
        assert_eq!(ms2.subtree_merges(), &[subtree_merge]);
        Ok(())
    }

    #[test]
    fn test_subtree_merges_with_url() -> Result<()> {
        let mut ms = MergeState::default();
        assert!(ms.subtree_merges().is_empty());

        let from_path = RepoPathBuf::from_string("foo/a".to_string())?;
        let to_path = RepoPathBuf::from_string("foo/b".to_string())?;
        let from_commit = HgId::from_byte_array([0x11; HgId::len()]);
        let from_url = Some("https://github.com/foo/bar.git".to_string());

        let subtree_merge = SubtreeMerge {
            from_commit: from_commit.clone(),
            from_path: from_path.clone(),
            to_path: to_path.clone(),
            from_url: from_url.clone(),
        };

        ms.add_subtree_merge(from_commit, from_path.clone(), to_path.clone(), from_url);
        assert_eq!(ms.subtree_merges(), &[subtree_merge.clone()]);

        let mut data = Vec::new();
        ms.serialize(&mut data)?;

        let mut buffer = &data[..];
        let ms2 = MergeState::deserialize(&mut buffer)?;
        assert_eq!(ms2.subtree_merges(), &[subtree_merge]);
        Ok(())
    }
}
