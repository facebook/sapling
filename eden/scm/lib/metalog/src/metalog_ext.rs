/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Extended APIs for application-specific metalog.
use std::collections::BTreeMap;

use anyhow::Result;
use refencode::RefName;
use types::HgId;
use types::Phase;

use crate::Id20;
use crate::MetaLog;

impl MetaLog {
    /// Decode bookmarks.
    pub fn get_bookmarks(&self) -> Result<BTreeMap<RefName, HgId>> {
        let decoded = match self.get("bookmarks")? {
            Some(data) => refencode::decode_bookmarks(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Decode (extra) git references like "refs/heads/main" => ...
    ///
    /// These are usually entries that are automatically translated to "visible heads" for dotgit
    /// mode. For dotsl mode, and other non-git configurations, this should be empty.
    ///
    /// These are tracked so we can update them during commit rewrites.
    pub fn get_git_refs(&self) -> Result<BTreeMap<RefName, HgId>> {
        let decoded = match self.get("gitrefs")? {
            // Same format as bookmarks.
            Some(data) => refencode::decode_bookmarks(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Decode remotenames.
    pub fn get_remotenames(&self) -> Result<BTreeMap<RefName, HgId>> {
        let decoded = match self.get("remotenames")? {
            Some(data) => refencode::decode_remotenames(&data)?,
            None => Default::default(),
        };
        Ok(decoded)
    }

    /// Decode remotename phases.
    pub fn get_remotename_phases(&self) -> Result<BTreeMap<RefName, Phase>> {
        let decoded = match self.get("remotename_phases")? {
            Some(data) => refencode::decode_remotename_phases(&data)?,
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
    pub fn set_bookmarks(&mut self, value: &BTreeMap<RefName, HgId>) -> Result<()> {
        let encoded = refencode::encode_bookmarks(value);
        self.set("bookmarks", &encoded)?;
        Ok(())
    }

    /// Update (extra) git references. This does not write to disk until `commit`.
    pub fn set_git_refs(&mut self, value: &BTreeMap<RefName, HgId>) -> Result<()> {
        let encoded = refencode::encode_bookmarks(value);
        self.set("gitrefs", &encoded)?;
        Ok(())
    }

    /// Update remotenames. This does not write to disk until `commit`.
    pub fn set_remotenames(&mut self, value: &BTreeMap<RefName, HgId>) -> Result<()> {
        let encoded = refencode::encode_remotenames(value);
        self.set("remotenames", &encoded)?;
        Ok(())
    }

    /// Update remotename phases. This does not write to disk until `commit`.
    pub fn set_remotename_phases(&mut self, value: &BTreeMap<RefName, Phase>) -> Result<()> {
        let encoded = refencode::encode_remotename_phases(value)?;
        self.set("remotename_phases", &encoded)?;
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
            None => Ok(None),
            Some(id) => Ok(Some(self.checkout(id)?)),
        }
    }
}
