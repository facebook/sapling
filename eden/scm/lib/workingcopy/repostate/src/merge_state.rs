/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::io::ErrorKind;
use std::io::Read;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use types::HgId;
use types::RepoPathBuf;

#[derive(Default, Debug)]
pub struct MergeState {
    // commits being merged
    local: Option<HgId>,
    other: Option<HgId>,

    // contextual labels for local/other/base
    labels: Vec<String>,

    // conflicting files
    files: HashMap<RepoPathBuf, FileInfo>,

    // merge driver definition at start of merge so we can detect merge driver
    // config changing during merge.
    merge_driver: Option<(String, MergeDriverState)>,

    // list of unsupported record types and accompanying record data, if any
    unsupported_records: Vec<(String, Vec<String>)>,
}

#[derive(Debug)]
pub struct UnsupportedMergeRecords(pub MergeState);

impl std::fmt::Display for UnsupportedMergeRecords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0.unsupported_records)
    }
}

impl MergeState {
    fn deserialize(data: &mut dyn Read) -> Result<Self> {
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
                            record_type: util::utf8::escape_non_utf8(&[record_type]),
                        },
                    );
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

    // Single byte record type as String, for convenience.
    record_type: String,
}

#[derive(Debug)]
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
}

#[derive(Debug)]
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
}
