// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Construct and serialize headers for bundle2 parts.

use std::ascii::AsciiExt;
use std::collections::HashMap;

use ascii::{AsciiStr, AsciiString, IntoAsciiString};
use bytes::{BigEndian, BufMut, Bytes};
use failure::SyncFailure;

use chunk::Chunk;
use errors::*;
use utils::BytesExt;

/// A bundle2 part header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PartHeader {
    part_type: AsciiString,
    part_type_lower: AsciiString,
    part_id: u32,
    // Part parameter keys are strings and values are arbitrary bytestrings
    // (which can even include null characters).
    mparams: HashMap<String, Bytes>,
    aparams: HashMap<String, Bytes>,
}

impl PartHeader {
    #[inline]
    pub fn part_type(&self) -> &AsciiStr {
        &self.part_type
    }

    #[inline]
    pub fn part_type_lower(&self) -> &AsciiStr {
        &self.part_type_lower
    }

    #[inline]
    pub fn part_id(&self) -> u32 {
        self.part_id
    }

    #[inline]
    pub fn mparams(&self) -> &HashMap<String, Bytes> {
        &self.mparams
    }

    #[inline]
    pub fn aparams(&self) -> &HashMap<String, Bytes> {
        &self.aparams
    }

    pub fn is_mandatory(&self) -> bool {
        // PartHeaderBuilder ensures that self.part_type is non-empty.
        self.part_type[0].is_uppercase()
    }

    pub fn encode(self) -> Chunk {
        let mut out_buf: Vec<u8> = Vec::new();

        // part type
        out_buf.put_u8(self.part_type.len() as u8);
        out_buf.put_slice(self.part_type.as_bytes());

        // part id
        out_buf.put_u32::<BigEndian>(self.part_id);

        // mandatory/advisory params
        let num_mparams = self.mparams.len() as u8;
        let num_aparams = self.aparams.len() as u8;

        out_buf.put_u8(num_mparams);
        out_buf.put_u8(num_aparams);

        // sort the params to ensure determinism
        let mut mparams: Vec<(String, Bytes)> = self.mparams.into_iter().collect();
        mparams.sort();
        let mut aparams: Vec<(String, Bytes)> = self.aparams.into_iter().collect();
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
    let part_type = header_bytes
        .drain_str(type_size)
        .with_context(|_| ErrorKind::Bundle2Decode("invalid part type".into()))?;

    let part_id = header_bytes.drain_u32();

    let nmparams = header_bytes.drain_u8() as usize;
    let naparams = header_bytes.drain_u8() as usize;

    let mut param_sizes = Vec::with_capacity(nmparams + naparams);
    let mut header = PartHeaderBuilder::with_capacity(part_type, nmparams, naparams)
        .with_context(|_| ErrorKind::Bundle2Decode("invalid part header".into()))?;

    for _ in 0..(nmparams + naparams) {
        // TODO: ensure none of the params is empty
        param_sizes.push((
            header_bytes.drain_u8() as usize,
            header_bytes.drain_u8() as usize,
        ));
    }

    for cur in 0..nmparams {
        let (ksize, vsize) = param_sizes[cur];
        let (key, val) = decode_header_param(&mut header_bytes, ksize, vsize).with_context(|_| {
            let err_msg = format!(
                "part '{}' (id {}): invalid param {}",
                header.part_type(),
                part_id,
                cur
            );
            ErrorKind::Bundle2Decode(err_msg)
        })?;
        header
            .add_mparam(key, val)
            .with_context(|_| ErrorKind::Bundle2Decode("invalid part header".into()))?;
    }

    for cur in nmparams..(nmparams + naparams) {
        let (ksize, vsize) = param_sizes[cur];
        let (key, val) = decode_header_param(&mut header_bytes, ksize, vsize).with_context(|_| {
            let err_msg = format!(
                "part '{}' (id {}): invalid param {}",
                header.part_type(),
                part_id,
                cur
            );
            ErrorKind::Bundle2Decode(err_msg)
        })?;
        header
            .add_aparam(key, val)
            .with_context(|_| ErrorKind::Bundle2Decode("invalid part header".into()))?;
    }

    Ok(header.build(part_id))
}

fn decode_header_param(buf: &mut Bytes, ksize: usize, vsize: usize) -> Result<(String, Bytes)> {
    let key = buf.drain_str(ksize).with_context(|_| "invalid key")?;
    let val = buf.split_to(vsize);
    return Ok((key, val));
}

/// Builder for a bundle2 part header.
#[derive(Debug, Eq, PartialEq)]
pub struct PartHeaderBuilder {
    part_type: AsciiString,
    mparams: HashMap<String, Bytes>,
    aparams: HashMap<String, Bytes>,
}

impl PartHeaderBuilder {
    pub fn new<S>(part_type: S) -> Result<Self>
    where
        S: IntoAsciiString + Send + 'static,
    {
        Self::with_capacity(part_type, 0, 0)
    }

    pub fn with_capacity<S>(
        part_type: S,
        mparam_capacity: usize,
        aparam_capacity: usize,
    ) -> Result<Self>
    where
        S: IntoAsciiString + Send + 'static,
    {
        let part_type = part_type
            .into_ascii_string()
            .map_err(|e| Error::from(SyncFailure::new(e)))
            .context("invalid part type")?;
        Self::check_part_type(&part_type)?;
        Ok(PartHeaderBuilder {
            part_type: part_type,
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
            bail!(
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
            bail!(
                "number of advisory params exceeds maximum {}",
                u8::max_value()
            );
        }
        self.aparams.insert(key, val);
        Ok(self)
    }

    pub fn part_type(&self) -> &AsciiStr {
        &self.part_type
    }

    /// Turn this `PartHeaderBuilder` into a `PartHeader`.
    ///
    /// We only accept part_id at this point because in the serialization use
    /// case, a part id is only assigned when the header is finalized.
    pub fn build(self, part_id: u32) -> PartHeader {
        let part_type_lower = self.part_type.to_ascii_lowercase();
        PartHeader {
            part_type: self.part_type,
            part_type_lower: part_type_lower,
            part_id: part_id,
            mparams: self.mparams,
            aparams: self.aparams,
        }
    }

    fn check_part_type(part_type: &AsciiStr) -> Result<()> {
        if part_type.is_empty() {
            bail!("part type empty");
        }
        if part_type.len() > u8::max_value() as usize {
            bail!(
                "part type '{}' exceeds max length {}",
                part_type,
                u8::max_value()
            );
        }
        Ok(())
    }

    fn check_param(&self, key: &str, val: &[u8]) -> Result<()> {
        if self.mparams.contains_key(key) || self.aparams.contains_key(key) {
            bail!(
                "part '{}': key '{}' already present in this part",
                self.part_type,
                key
            );
        }
        if key.is_empty() {
            bail!("part '{}': empty key", self.part_type);
        }
        if key.len() > u8::max_value() as usize {
            bail!(
                "part '{}': key '{}' exceeds max length {}",
                self.part_type,
                key,
                u8::max_value()
            );
        }
        if val.is_empty() {
            bail!(
                "part '{}': value for key '{}' is empty",
                self.part_type,
                key
            );
        }
        if val.len() > u8::max_value() as usize {
            bail!(
                "part '{}': value for key '{}' exceeds max length {}",
                self.part_type,
                key,
                u8::max_value()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use quickcheck::{quickcheck, TestResult};

    use super::*;
    use quickcheck_types::QCBytes;

    const MAX_LEN: usize = ::std::u8::MAX as usize;

    #[test]
    fn test_check_part_type() {
        assert_part_type("", false);
        assert_part_type("abc", true);
        assert_part_type("a".repeat(MAX_LEN), true);
        assert_part_type("a".repeat(MAX_LEN + 1), false);
    }

    #[test]
    fn test_check_params() {
        let mut header = PartHeaderBuilder::new("test").unwrap();

        assert_param(&mut header, "", &b"val"[..], false);
        assert_param(&mut header, "key", &b""[..], false);
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
                as fn(AsciiString, u32, HashMap<String, QCBytes>, HashMap<String, QCBytes>)
                    -> TestResult,
        );
    }

    fn roundtrip(
        part_type: AsciiString,
        part_id: u32,
        mparams: HashMap<String, QCBytes>,
        aparams: HashMap<String, QCBytes>,
    ) -> TestResult {
        match roundtrip_inner(part_type, part_id, mparams, aparams) {
            Ok(test_result) => test_result,
            Err(_err) => TestResult::discard(),
        }
    }

    /// Test that roundtrip encoding -> decoding works.
    ///
    /// For convenience, errors here are treated as skipped tests. Panics are
    /// test failures.
    fn roundtrip_inner(
        part_type: AsciiString,
        part_id: u32,
        mparams: HashMap<String, QCBytes>,
        aparams: HashMap<String, QCBytes>,
    ) -> Result<TestResult> {
        let mut builder = PartHeaderBuilder::new(part_type)?;
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

    fn assert_part_type<S>(part_type: S, valid: bool)
    where
        S: IntoAsciiString + Send + 'static,
    {
        let header = PartHeaderBuilder::new(part_type);
        if valid {
            header.unwrap();
        } else {
            header.unwrap_err();
        }
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
