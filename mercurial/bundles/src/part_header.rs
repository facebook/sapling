/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Construct and serialize headers for bundle2 parts.

use std::collections::HashMap;

use bytes::{BufMut, Bytes};
use failure_ext::bail_msg;
use quickcheck::{Arbitrary, Gen};
use rand::seq::SliceRandom;

use crate::chunk::Chunk;
use crate::errors::*;
use crate::utils::BytesExt;

pub type PartId = u32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PartHeaderType {
    /// Responsible for sending Changesets and Filelogs during a push. In Mercurial it also sends
    /// flat Manifests, but with Mononoke we support only TreeManifests.
    Changegroup,
    /// When responding for bundle2 this part contains the response for the corresponding
    /// Changegroup.
    ReplyChangegroup,
    /// The bundle2 sender describes his own capabilities to handle the bundle2 that will be send
    /// in response for this one.
    Replycaps,
    /// Contains keys that are being used in this bundle2, f.e. bookmarks
    Listkeys,
    /// Contains wirepacks that are encoded TreeManifests required in the push.
    B2xTreegroup2,
    /// In Mercurial this is used to verify that during the push the heads did not change. In
    /// Mononoke this parameter will be ignored, because it does not provide transaction during
    /// push
    CheckHeads,
    /// Contains list of heads that are present client-side and server-side.
    /// Used in pushrebase to find out what to send to the client.
    B2xCommonHeads,
    /// Contains changegroup for infinitepush commits
    B2xInfinitepush,
    /// Contains bookmarks for infinitepush backups (won't be used in Mononoke,
    /// but they needs to be parsed).
    B2xInfinitepushBookmarks,
    /// Pushrebase part with changegroup
    B2xRebase,
    /// Pushrebase part that contains packs
    B2xRebasePack,
    /// Pushkey part is used to update different namespaces: phases, bookmarks, etc.
    /// In Mononoke it's used to update bookmarks.
    Pushkey,
    /// Respond to a corresponding pushkey part
    ReplyPushkey,
    /// Contains parameters that can be used by hooks
    Pushvars,
    /// Contains phase heads part
    /// Used in communicating phases between Mononoke and clients
    /// Pushkey / Listkeys are not used to communicate phases
    PhaseHeads,
    // RemoteChangegroup,       // We don't wish to support this functionality
    // CheckBookmarks,          // TODO Do we want to support this?
    // CheckHeads,              // TODO Do we want to support this?
    // CheckUpdatedHeads,       // TODO Do we want to support this?
    // CheckPhases,             // TODO Do we want to support this?
    // Output,                  // TODO Do we want to support this?
    // ErrorAbort,              // TODO Do we want to support this?
    // ErrorPushkey,            // TODO Do we want to support this?
    // ErrorUnsupportedContent, // TODO Do we want to support this?
    // ErrorPushRaced,          // TODO Do we want to support this?
    // Pushkey,                 // TODO Do we want to support this?
    // Bookmarks,               // TODO Do we want to support this?
    // ReplyPushkey,            // TODO Do we want to support this?
    Obsmarkers,
    // ReplyObsmarkers,         // TODO Do we want to support this?
    // HgtagsFnodes,            // TODO Do we want to support this?
}

impl PartHeaderType {
    fn decode(data: &str) -> Result<Self> {
        use self::PartHeaderType::*;
        match data.to_ascii_lowercase().as_str() {
            "changegroup" => Ok(Changegroup),
            "reply:changegroup" => Ok(ReplyChangegroup),
            "replycaps" => Ok(Replycaps),
            "listkeys" => Ok(Listkeys),
            "b2x:treegroup2" => Ok(B2xTreegroup2),
            "b2x:infinitepush" => Ok(B2xInfinitepush),
            "b2x:infinitepushscratchbookmarks" => Ok(B2xInfinitepushBookmarks),
            "b2x:commonheads" => Ok(B2xCommonHeads),
            "b2x:rebase" => Ok(B2xRebase),
            "b2x:rebasepackpart" => Ok(B2xRebasePack),
            "check:heads" => Ok(CheckHeads),
            "pushkey" => Ok(Pushkey),
            "reply:pushkey" => Ok(ReplyPushkey),
            "pushvars" => Ok(Pushvars),
            "phase-heads" => Ok(PhaseHeads),
            "obsmarkers" => Ok(Obsmarkers),
            bad => bail_msg!("unknown header type {}", bad),
        }
    }

    fn as_str(&self) -> &str {
        use self::PartHeaderType::*;
        match *self {
            Changegroup => "changegroup",
            ReplyChangegroup => "reply:changegroup",
            Replycaps => "replycaps",
            Listkeys => "listkeys",
            B2xTreegroup2 => "b2x:treegroup2",
            B2xCommonHeads => "b2x:commonheads",
            B2xInfinitepush => "b2x:infinitepush",
            B2xInfinitepushBookmarks => "b2x:infinitepushscratchbookmarks",
            B2xRebase => "b2x:rebase",
            B2xRebasePack => "b2x:rebasepackpart",
            CheckHeads => "check:heads",
            Pushkey => "pushkey",
            Pushvars => "pushvars",
            ReplyPushkey => "reply:pushkey",
            PhaseHeads => "phase-heads",
            Obsmarkers => "obsmarkers",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartHeaderInner {
    pub part_type: PartHeaderType,
    pub mandatory: bool,
    pub part_id: PartId,
    // Part parameter keys are strings and values are arbitrary bytestrings
    // (which can even include null characters).
    pub mparams: HashMap<String, Bytes>,
    pub aparams: HashMap<String, Bytes>,
}

/// A bundle2 part header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartHeader(PartHeaderInner);

impl PartHeader {
    #[inline]
    pub fn part_type(&self) -> &PartHeaderType {
        &self.0.part_type
    }

    #[inline]
    pub fn part_id(&self) -> PartId {
        self.0.part_id
    }

    #[inline]
    pub fn mparams(&self) -> &HashMap<String, Bytes> {
        &self.0.mparams
    }

    #[inline]
    pub fn aparams(&self) -> &HashMap<String, Bytes> {
        &self.0.aparams
    }

    pub fn mandatory(&self) -> bool {
        self.0.mandatory
    }

    pub fn into_inner(self) -> PartHeaderInner {
        self.0
    }

    pub fn encode(self) -> Chunk {
        let mut out_buf: Vec<u8> = Vec::new();

        // part type
        let part_type = self.0.part_type.as_str();
        let part_type = if self.0.mandatory {
            part_type.to_ascii_uppercase()
        } else {
            part_type.to_owned()
        };
        let part_type = part_type.as_bytes();
        out_buf.put_u8(part_type.len() as u8);
        out_buf.put_slice(part_type);

        // part id
        out_buf.put_u32_be(self.0.part_id);

        // mandatory/advisory params
        let num_mparams = self.0.mparams.len() as u8;
        let num_aparams = self.0.aparams.len() as u8;

        out_buf.put_u8(num_mparams);
        out_buf.put_u8(num_aparams);

        // sort the params to ensure determinism
        let mut mparams: Vec<(String, Bytes)> = self.0.mparams.into_iter().collect();
        mparams.sort();
        let mut aparams: Vec<(String, Bytes)> = self.0.aparams.into_iter().collect();
        aparams.sort();

        // param sizes
        for &(ref key, ref val) in &mparams {
            out_buf.put_u8(key.len() as u8);
            out_buf.put_u8(val.len() as u8);
        }
        for &(ref key, ref val) in &aparams {
            out_buf.put_u8(key.len() as u8);
            out_buf.put_u8(val.len() as u8);
        }

        // the actual params themselves
        for &(ref key, ref val) in &mparams {
            out_buf.put_slice(key.as_bytes());
            out_buf.put_slice(&val);
        }
        for &(ref key, ref val) in &aparams {
            out_buf.put_slice(key.as_bytes());
            out_buf.put_slice(&val);
        }

        // This can only fail because the chunk is too big, but the restrictions
        // on sizes above put a cap on the size of the chunk that's much
        // smaller.
        Chunk::new(out_buf).expect("chunk cannot be too big")
    }
}

pub fn decode(mut header_bytes: Bytes) -> Result<PartHeader> {
    // Header internals:
    // ---
    // type_size: u8
    // part_type: str (type_size bytes)
    // part_id: u32
    // number of mandatory params: u8
    // number of advisory params: u8
    // param_sizes: (key: u8, val: u8) * (number of mandatory + advisory params)
    // parameters: key, val are both strings of lengths corresponding to index in param_sizes
    // ---
    // This function assumes that the full header is available.
    let type_size = header_bytes.drain_u8() as usize;
    let part_type_encoded = header_bytes
        .drain_str(type_size)
        .with_context(|| ErrorKind::Bundle2Decode("invalid part type".into()))?;
    let part_type = PartHeaderType::decode(&part_type_encoded)?;

    let mandatory = part_type_encoded.chars().any(|c| c.is_ascii_uppercase());

    let part_id = header_bytes.drain_u32();

    let nmparams = header_bytes.drain_u8() as usize;
    let naparams = header_bytes.drain_u8() as usize;

    let mut param_sizes = Vec::with_capacity(nmparams + naparams);
    let mut header = PartHeaderBuilder::with_capacity(part_type, mandatory, nmparams, naparams)
        .with_context(|| ErrorKind::Bundle2Decode("invalid part header".into()))?;

    for _ in 0..(nmparams + naparams) {
        // TODO: ensure none of the params is empty
        param_sizes.push((
            header_bytes.drain_u8() as usize,
            header_bytes.drain_u8() as usize,
        ));
    }

    for cur in 0..nmparams {
        let (ksize, vsize) = param_sizes[cur];
        let (key, val) =
            decode_header_param(&mut header_bytes, ksize, vsize).with_context(|| {
                let err_msg = format!(
                    "part '{:?}' (id {}): invalid param {}",
                    header.part_type(),
                    part_id,
                    cur
                );
                ErrorKind::Bundle2Decode(err_msg)
            })?;
        header
            .add_mparam(key, val)
            .with_context(|| ErrorKind::Bundle2Decode("invalid part header".into()))?;
    }

    for cur in nmparams..(nmparams + naparams) {
        let (ksize, vsize) = param_sizes[cur];
        let (key, val) =
            decode_header_param(&mut header_bytes, ksize, vsize).with_context(|| {
                let err_msg = format!(
                    "part '{:?}' (id {}): invalid param {}",
                    header.part_type(),
                    part_id,
                    cur
                );
                ErrorKind::Bundle2Decode(err_msg)
            })?;
        header
            .add_aparam(key, val)
            .with_context(|| ErrorKind::Bundle2Decode("invalid part header".into()))?;
    }

    Ok(header.build(part_id))
}

fn decode_header_param(buf: &mut Bytes, ksize: usize, vsize: usize) -> Result<(String, Bytes)> {
    let key = buf.drain_str(ksize).context("invalid key")?;
    let val = buf.split_to(vsize);
    return Ok((key, val));
}

/// Builder for a bundle2 part header.
#[derive(Debug, Eq, PartialEq)]
pub struct PartHeaderBuilder {
    part_type: PartHeaderType,
    mandatory: bool,
    mparams: HashMap<String, Bytes>,
    aparams: HashMap<String, Bytes>,
}

impl PartHeaderBuilder {
    pub fn new(part_type: PartHeaderType, mandatory: bool) -> Result<Self> {
        Self::with_capacity(part_type, mandatory, 0, 0)
    }

    pub fn with_capacity(
        part_type: PartHeaderType,
        mandatory: bool,
        mparam_capacity: usize,
        aparam_capacity: usize,
    ) -> Result<Self> {
        Ok(PartHeaderBuilder {
            part_type,
            mandatory,
            mparams: HashMap::with_capacity(mparam_capacity),
            aparams: HashMap::with_capacity(aparam_capacity),
        })
    }

    pub fn add_mparam<S, B>(&mut self, key: S, val: B) -> Result<&mut Self>
    where
        S: Into<String>,
        B: Into<Bytes>,
    {
        let key = key.into();
        let val = val.into();
        self.check_param(&key, &val)?;
        if self.mparams.len() >= u8::max_value() as usize {
            bail_msg!(
                "number of mandatory params exceeds maximum {}",
                u8::max_value()
            );
        }
        self.mparams.insert(key, val);
        Ok(self)
    }

    pub fn add_aparam<S, B>(&mut self, key: S, val: B) -> Result<&mut Self>
    where
        S: Into<String>,
        B: Into<Bytes>,
    {
        let key = key.into();
        let val = val.into();
        self.check_param(&key, &val)?;
        if self.aparams.len() >= u8::max_value() as usize {
            bail_msg!(
                "number of advisory params exceeds maximum {}",
                u8::max_value()
            );
        }
        self.aparams.insert(key, val);
        Ok(self)
    }

    pub fn part_type(&self) -> &PartHeaderType {
        &self.part_type
    }

    /// Turn this `PartHeaderBuilder` into a `PartHeader`.
    ///
    /// We only accept part_id at this point because in the serialization use
    /// case, a part id is only assigned when the header is finalized.
    pub fn build(self, part_id: PartId) -> PartHeader {
        PartHeader(PartHeaderInner {
            part_type: self.part_type,
            mandatory: self.mandatory,
            part_id,
            mparams: self.mparams,
            aparams: self.aparams,
        })
    }

    fn check_param(&self, key: &str, val: &[u8]) -> Result<()> {
        if self.mparams.contains_key(key) || self.aparams.contains_key(key) {
            bail_msg!(
                "part '{:?}': key '{}' already present in this part",
                self.part_type,
                key
            );
        }
        if key.is_empty() {
            bail_msg!("part '{:?}': empty key", self.part_type);
        }
        if key.len() > u8::max_value() as usize {
            bail_msg!(
                "part '{:?}': key '{}' exceeds max length {}",
                self.part_type,
                key,
                u8::max_value()
            );
        }
        if val.len() > u8::max_value() as usize {
            bail_msg!(
                "part '{:?}': value for key '{}' exceeds max length {}",
                self.part_type,
                key,
                u8::max_value()
            );
        }
        Ok(())
    }
}

impl Arbitrary for PartHeaderType {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use self::PartHeaderType::*;
        [
            Changegroup,
            ReplyChangegroup,
            Replycaps,
            Listkeys,
            B2xTreegroup2,
            CheckHeads,
        ]
        .choose(g)
        .expect("empty choice provided")
        .clone()
    }
}

#[cfg(test)]
mod test {
    use quickcheck::{quickcheck, TestResult};

    use super::*;
    use crate::quickcheck_types::QCBytes;

    const MAX_LEN: usize = ::std::u8::MAX as usize;

    #[test]
    fn test_check_params() {
        let mut header = PartHeaderBuilder::new(PartHeaderType::Changegroup, false).unwrap();

        assert_param(&mut header, "", &b"val"[..], false);
        assert_param(&mut header, "key", &b"val"[..], true);
        // if a key was already stored, reject it the second time
        assert_param(&mut header, "key", &b"val"[..], false);

        assert_param(&mut header, "k".repeat(MAX_LEN), &b"val"[..], true);
        assert_param(&mut header, "k".repeat(MAX_LEN + 1), &b"val"[..], false);
        assert_param(&mut header, "key2", "v".repeat(MAX_LEN), true);
        assert_param(&mut header, "key3", "v".repeat(MAX_LEN + 1), false);
    }

    #[test]
    fn test_roundtrip() {
        quickcheck(
            roundtrip
                as fn(
                    PartHeaderType,
                    bool,
                    PartId,
                    HashMap<String, QCBytes>,
                    HashMap<String, QCBytes>,
                ) -> TestResult,
        );
    }

    fn roundtrip(
        part_type: PartHeaderType,
        mandatory: bool,
        part_id: PartId,
        mparams: HashMap<String, QCBytes>,
        aparams: HashMap<String, QCBytes>,
    ) -> TestResult {
        match roundtrip_inner(part_type, mandatory, part_id, mparams, aparams) {
            Ok(test_result) => test_result,
            Err(_err) => TestResult::discard(),
        }
    }

    /// Test that roundtrip encoding -> decoding works.
    ///
    /// For convenience, errors here are treated as skipped tests. Panics are
    /// test failures.
    fn roundtrip_inner(
        part_type: PartHeaderType,
        mandatory: bool,
        part_id: PartId,
        mparams: HashMap<String, QCBytes>,
        aparams: HashMap<String, QCBytes>,
    ) -> Result<TestResult> {
        let mut builder = PartHeaderBuilder::new(part_type, mandatory)?;
        for (k, v) in mparams {
            builder.add_mparam(k, v)?;
        }
        for (k, v) in aparams {
            builder.add_aparam(k, v)?;
        }

        let header = builder.build(part_id);

        let header_chunk = header.clone().encode();
        let header_bytes = header_chunk.into_bytes().unwrap();
        let decoded_header = decode(header_bytes).unwrap();

        assert_eq!(header, decoded_header);

        Ok(TestResult::passed())
    }

    fn assert_param<S: Into<String>, B: Into<Bytes>>(
        header: &mut PartHeaderBuilder,
        key: S,
        val: B,
        valid: bool,
    ) {
        let res = header.add_aparam(key, val);
        if valid {
            res.unwrap();
        } else {
            res.unwrap_err();
        }
    }
}
