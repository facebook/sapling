/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Tracking of commit mutations (amends, rebases, etc.)

use std::io::{Read, Write};

use anyhow::{anyhow, Result};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use vlqencoding::{VLQDecode, VLQEncode};

use crate::node::{Node, ReadNodeExt, WriteNodeExt};

#[derive(Clone, PartialEq, PartialOrd)]
pub struct MutationEntry {
    pub succ: Node,
    pub preds: Vec<Node>,
    pub split: Vec<Node>,
    pub op: String,
    pub user: String,
    pub time: i64,
    pub tz: i32,
    pub extra: Vec<(Box<[u8]>, Box<[u8]>)>,
}

/// Default size for a buffer that will be used for serializing a mutation entry.
///
/// This is:
///   * Version (1 byte)
///   * Successor hash (20 bytes)
///   * Predecessor count (1 byte)
///   * Predecessor hash (20 bytes) - most entries have only one predecessor
///   * Operation (~7 bytes) - e.g. amend, rebase
///   * User (~40 bytes)
///   * Time (4-8 bytes)
///   * Timezone (~2 bytes)
///   * Extra count (1 byte)
pub const DEFAULT_ENTRY_SIZE: usize = 100;

const DEFAULT_VERSION: u8 = 1;

impl MutationEntry {
    pub fn serialize(&self, w: &mut dyn Write) -> Result<()> {
        w.write_u8(DEFAULT_VERSION)?;
        w.write_node(&self.succ)?;
        w.write_vlq(self.preds.len())?;
        for pred in self.preds.iter() {
            w.write_node(pred)?;
        }
        w.write_vlq(self.split.len())?;
        for split in self.split.iter() {
            w.write_node(split)?;
        }
        w.write_vlq(self.op.len())?;
        w.write_all(self.op.as_bytes())?;
        w.write_vlq(self.user.len())?;
        w.write_all(&self.user.as_bytes())?;
        w.write_f64::<BigEndian>(self.time as f64)?;
        w.write_vlq(self.tz)?;
        w.write_vlq(self.extra.len())?;
        for (key, value) in self.extra.iter() {
            w.write_vlq(key.len())?;
            w.write_all(&key)?;
            w.write_vlq(value.len())?;
            w.write_all(&value)?;
        }
        Ok(())
    }

    pub fn deserialize(r: &mut dyn Read) -> Result<Self> {
        enum EntryFormat {
            FloatDate,
            Latest,
        }
        let format = match r.read_u8()? {
            0 => return Err(anyhow!("invalid mutation entry version: 0")),
            1..=4 => {
                // These versions stored the date as an f64.
                EntryFormat::FloatDate
            }
            5 => EntryFormat::Latest,
            v => return Err(anyhow!("unsupported mutation entry version: {}", v)),
        };
        let succ = r.read_node()?;
        let pred_count = r.read_vlq()?;
        let mut preds = Vec::with_capacity(pred_count);
        for _ in 0..pred_count {
            preds.push(r.read_node()?);
        }
        let split_count = r.read_vlq()?;
        let mut split = Vec::with_capacity(split_count);
        for _ in 0..split_count {
            split.push(r.read_node()?);
        }
        let op_len = r.read_vlq()?;
        let mut op = vec![0; op_len];
        r.read_exact(&mut op)?;
        let op = String::from_utf8(op)?;
        let user_len = r.read_vlq()?;
        let mut user = vec![0; user_len];
        r.read_exact(&mut user)?;
        let user = String::from_utf8(user)?;
        let time = match format {
            EntryFormat::FloatDate => {
                // The date was stored as a floating point number.  We
                // actually want an integer, so truncate and convert.
                r.read_f64::<BigEndian>()?.trunc() as i64
            }
            _ => r.read_vlq()?,
        };
        let tz = r.read_vlq()?;
        let extra_count = r.read_vlq()?;
        let mut extra = Vec::with_capacity(extra_count);
        for _ in 0..extra_count {
            let key_len = r.read_vlq()?;
            let mut key = vec![0; key_len];
            r.read_exact(&mut key)?;
            let value_len = r.read_vlq()?;
            let mut value = vec![0; value_len];
            r.read_exact(&mut value)?;
            extra.push((key.into_boxed_slice(), value.into_boxed_slice()));
        }
        Ok(MutationEntry {
            succ,
            preds,
            split,
            op,
            user,
            time,
            tz,
            extra,
        })
    }

    /// Return true if this represents an 1:1 commit replacement.
    /// A split or a fold are not 1:1 replacement.
    ///
    /// Note: Resolving divergence should use multiple 1:1 records
    /// and are considered 1:1 replacements.
    ///
    /// If this function returns true, `preds` is ensured to only
    /// have one item.
    pub fn is_one_to_one(&self) -> bool {
        self.split.is_empty() && self.preds.len() == 1
    }
}
