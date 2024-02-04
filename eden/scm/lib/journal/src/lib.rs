/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Write;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use hgtime::HgTime;
use types::HgId;

/// Individual journal entry
///
/// - timestamp: a mercurial (time, timezone) tuple
/// - user: the username that ran the command
/// - command: the @prog@ command that triggered this record
/// - namespace: the entry namespace, an opaque string
/// - name: the name of the changed item, opaque string with meaning in the namespace
/// - oldhashes: a tuple of one or more binary hashes for the old location
/// - newhashes: a tuple of one or more binary hashes for the new location
///
/// Handles serialisation from and to the storage format. Fields are
/// separated by newlines, hashes are written out in hex separated by commas,
/// timestamp and timezone are separated by a space.
pub struct JournalEntry {
    pub timestamp: HgTime,
    pub user: String,
    pub command: String,
    pub namespace: String,
    pub name: String,
    pub old_hashes: Vec<HgId>,
    pub new_hashes: Vec<HgId>,
}

impl JournalEntry {
    pub fn deserialize(data: &[u8]) -> Result<Self> {
        let line = String::from_utf8(data.to_vec()).with_context(|| {
            format!(
                "decoding journal entry {}",
                util::utf8::escape_non_utf8(data),
            )
        })?;
        let elems = line.split('\n').collect::<Vec<_>>();
        let [time, user, command, namespace, name, old_hashes, new_hashes] = *elems.as_slice()
        else {
            bail!(
                "journal entry '{}' contains incorrect number of elements",
                line
            );
        };
        let timestamp = HgTime::parse_hg_internal_format(time)
            .flatten()
            .with_context(|| format!("unable to parse timestamp from journal line '{}'", line))?;
        let old_hashes = parse_hashes(line.as_str(), old_hashes)?;
        let new_hashes = parse_hashes(line.as_str(), new_hashes)?;
        Ok(Self {
            timestamp,
            user: user.to_owned(),
            command: command.to_owned(),
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            old_hashes,
            new_hashes,
        })
    }

    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut buf = String::new();
        write!(
            buf,
            r#"{} {}
{}
{}
{}
{}
"#,
            self.timestamp.unixtime,
            self.timestamp.offset,
            self.user.as_str(),
            self.command.as_str(),
            self.namespace.as_str(),
            self.name.as_str()
        )?;
        for (idx, id) in self.old_hashes.iter().enumerate() {
            if idx > 0 {
                buf.write_str(",")?;
            }
            write!(buf, "{}", id)?;
        }
        write!(buf, "\n")?;
        for (idx, id) in self.new_hashes.iter().enumerate() {
            if idx > 0 {
                buf.write_str(",")?;
            }
            write!(buf, "{}", id)?;
        }
        Ok(buf.into_bytes())
    }
}

fn parse_hashes(line: &str, hashes: &str) -> Result<Vec<HgId>> {
    hashes
        .split(',')
        .map(HgId::from_str)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("unable to parse hashes from journal line '{}'", line))
}
