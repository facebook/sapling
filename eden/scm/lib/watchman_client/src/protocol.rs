/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::*;
use failure::Fallible;
use std::io::{BufRead, Read, Write};

pub trait Protocol {
    fn read<T, R>(r: &mut R) -> Fallible<T>
    where
        for<'de> T: serde::Deserialize<'de>,
        R: BufRead;
    fn write<T: ?Sized, W>(w: &mut W, value: &T) -> Fallible<()>
    where
        T: serde::Serialize,
        W: Write;
    /// protocol name
    fn name() -> &'static str;
}

/// Implementations:

pub struct JsonProtocol;

impl JsonProtocol {
    fn delimiter() -> u8 {
        b'\n'
    }
}

impl Protocol for JsonProtocol {
    fn read<T, R>(r: &mut R) -> Fallible<T>
    where
        for<'de> T: serde::Deserialize<'de>,
        R: BufRead,
    {
        let mut buffer = Vec::new();
        r.read_until(JsonProtocol::delimiter(), &mut buffer)?;
        let resp: T = serde_json::from_slice(&buffer)?;
        Ok(resp)
    }
    fn write<T: ?Sized, W>(w: &mut W, value: &T) -> Fallible<()>
    where
        T: serde::Serialize,
        W: Write,
    {
        w.write_all(&serde_json::to_vec(value)?)?;
        w.write(&[JsonProtocol::delimiter()])?;
        w.flush()?;
        Ok(())
    }
    fn name() -> &'static str {
        "json"
    }
}

pub struct BserProtocol;
impl Protocol for BserProtocol {
    fn read<T, R>(r: &mut R) -> Fallible<T>
    where
        for<'de> T: serde::Deserialize<'de>,
        R: BufRead,
    {
        let resp: T = serde_bser::from_reader(r as &mut dyn Read)
            .map_err(|e| ErrorKind::WatchmanBserParsingError(format!("{}", e)))?;
        Ok(resp)
    }
    fn write<T: ?Sized, W>(w: &mut W, value: &T) -> Fallible<()>
    where
        T: serde::Serialize,
        W: Write,
    {
        let w = serde_bser::ser::serialize(w, value)
            .map_err(|e| ErrorKind::WatchmanBserParsingError(format!("{}", e)))?;
        w.flush()?;
        Ok(())
    }
    fn name() -> &'static str {
        "bser-v2"
    }
}

/// Specific for protocols

impl JsonProtocol {
    pub fn to_string<T: ?Sized>(value: &T) -> Fallible<String>
    where
        T: serde::Serialize,
    {
        Ok(serde_json::to_string(value)?)
    }
    pub fn to_string_pretty<T: ?Sized>(value: &T) -> Fallible<String>
    where
        T: serde::Serialize,
    {
        Ok(serde_json::to_string_pretty(value)?)
    }
}
