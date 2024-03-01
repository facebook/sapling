/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Extended APIs for application-specific metalog.
use std::collections::BTreeMap;

use anyhow::Result;
use types::HgId;

use crate::Id20;
use crate::MetaLog;

impl MetaLog {
    /// Decode bookmarks.
    pub fn get_bookmarks(&self) -> Result<BTreeMap<String, HgId>> {
        let decoded = match self.get("bookmarks")? {
            Some(data) => refencode::decode_bookmarks(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Decode remotenames.
    pub fn get_remotenames(&self) -> Result<BTreeMap<String, HgId>> {
        let decoded = match self.get("remotenames")? {
            Some(data) => refencode::decode_remotenames(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Decode visibleheads.
    pub fn get_visibleheads(&self) -> Result<Vec<HgId>> {
        let decoded = match self.get("visibleheads")? {
            Some(data) => refencode::decode_visibleheads(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Update bookmarks. This does not write to disk until `commit`.
    pub fn set_bookmarks(&mut self, value: &BTreeMap<String, HgId>) -> Result<()> {
        let encoded = refencode::encode_bookmarks(value);
        self.set("bookmarks", &encoded)?;
        Ok(())
    }

    /// Update remotenames. This does not write to disk until `commit`.
    pub fn set_remotenames(&mut self, value: &BTreeMap<String, HgId>) -> Result<()> {
        let encoded = refencode::encode_remotenames(value);
        self.set("remotenames", &encoded)?;
        Ok(())
    }

    /// Update visibleheads. This does not write to disk until `commit`.
    pub fn set_visibleheads(&mut self, value: &[HgId]) -> Result<()> {
        let encoded = refencode::encode_visibleheads(value);
        self.set("visibleheads", &encoded)?;
        Ok(())
    }

    /// Checkout the "parent" metalog. Not all entries have "parent".
    /// The "parent" is defined by "Parent: HASH" in the message,
    /// written by the transaction layer.
    pub fn parent(&self) -> Result<Option<Self>> {
        let parent_id = self.message().lines().find_map(|l| {
            let rest = l.strip_prefix("Parent: ")?;
            Id20::from_hex(rest.as_bytes()).ok()
        });
        match parent_id {
            None => return Ok(None),
            Some(id) => Ok(Some(self.checkout(id)?)),
        }
    }
}
