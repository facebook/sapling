/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use hgtime::HgTime;
use repolock::try_lock_with_contents;
use types::HgId;

const JOURNAL_FILENAME: &str = "namejournal";
const JOURNAL_LOCK_FILENAME: &str = "namejournal.lock";
const JOURNAL_FORMAT_VERSION: &str = "0";

pub struct Journal {
    dot_hg_path: PathBuf,
}

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

    pub fn serialize(&self, buf: &mut Vec<u8>) -> Result<()> {
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
                write!(buf, ",")?;
            }
            write!(buf, "{}", id)?;
        }
        write!(buf, "\n")?;
        for (idx, id) in self.new_hashes.iter().enumerate() {
            if idx > 0 {
                write!(buf, ",")?;
            }
            write!(buf, "{}", id)?;
        }
        Ok(())
    }
}

fn parse_hashes(line: &str, hashes: &str) -> Result<Vec<HgId>> {
    hashes
        .split(',')
        .map(HgId::from_str)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("unable to parse hashes from journal line '{}'", line))
}

impl Journal {
    pub fn open(dot_hg_path: PathBuf) -> Result<Self> {
        Ok(Self { dot_hg_path })
    }

    /// Record a new journal entry
    ///
    /// - `namespace`: an opaque string; this can be used to filter on the type of recorded entries.
    /// - `name`: the name defining this entry; for bookmarks, this is the bookmark name. Can be filtered on when retrieving entries.
    /// - `old_hash` and `new_hash`: lists of commit hashes. These represent the old and new position of the named item.
    pub fn record_new_entry(
        &self,
        raw_args: &[String],
        namespace: &str,
        name: &str,
        old_hashes: &[HgId],
        new_hashes: &[HgId],
    ) -> Result<()> {
        let command = util::sys::shell_escape(raw_args);
        let timestamp = hgtime::HgTime::now()
            .context("unable to determine current time when writing to journal")?;
        let user = util::sys::username()?;
        let command = if let Some((left, _)) = command.split_once('\n') {
            format!("{} ...", left)
        } else {
            command.to_owned()
        };
        let entry = JournalEntry {
            timestamp,
            user,
            command,
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            old_hashes: old_hashes.to_vec(),
            new_hashes: new_hashes.to_vec(),
        };
        let _journal_lock =
            try_lock_with_contents(self.dot_hg_path.as_path(), JOURNAL_LOCK_FILENAME)?;
        let journal_file_path = self.dot_hg_path.join(JOURNAL_FILENAME);
        let mut data = if journal_file_path.exists() {
            vec![]
        } else {
            // TODO(sggutier): in in theory the journal version could change and we should check that,
            // but let's not worry about that right now
            let mut d = JOURNAL_FORMAT_VERSION.as_bytes().to_vec();
            d.push(0u8);
            d
        };
        entry.serialize(&mut data)?;
        data.push(0u8);
        let mut journal_file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(journal_file_path)?;
        journal_file.write_all(data.as_slice())?;
        Ok(())
    }
}
