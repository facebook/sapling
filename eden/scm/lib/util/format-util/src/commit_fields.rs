/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::Result;
use minibytes::Text;
use storemodel::SerializationFormat;
use types::Id20;

/// Extract commit fields from commit raw text.
/// Note: "parents" only exists in the Git commit raw text.
pub trait CommitFields: Send + 'static {
    /// Root tree SHA1.
    fn root_tree(&self) -> Result<Id20>;

    /// Author name and email, like "Foo bar <foo@example.com>"
    fn author_name(&self) -> Result<&str>;

    /// Committer name and email, like "Foo bar <foo@example.com>"
    /// Returns `None` if committer is not explicitly tracked
    /// (i.e. hg format without committer_date extra).
    fn committer_name(&self) -> Result<Option<&str>>;

    /// Author (creation) date.
    /// (UTC seconds since UNIX epoch, timezone offset in seconds)
    fn author_date(&self) -> Result<(u64, i32)>;

    /// Committer (modified) date.
    /// Returns `None` if committer is not explicitly tracked
    /// (i.e. hg format without committer_date extra).
    /// (UTC seconds since UNIX epoch, timezone offset in seconds)
    fn committer_date(&self) -> Result<Option<(u64, i32)>>;

    /// Parent information. Order-preserved.
    /// Returns `None` if not tracked in the commit text (i.e. hg format).
    fn parents(&self) -> Result<Option<&[Id20]>> {
        Ok(None)
    }

    /// Changed files list, separated by space.
    /// Returns `None` if not tracked in the commit text (i.e. git format).
    fn files(&self) -> Result<Option<&[Text]>> {
        Ok(None)
    }

    /// Returns `None` if the commit does not have extras.
    fn extras(&self) -> Result<Option<&BTreeMap<Text, Text>>> {
        Ok(None)
    }

    /// Commit message encoded in UTF-8.
    fn description(&self) -> Result<&str>;

    /// Format of the commit.
    fn format(&self) -> SerializationFormat;

    /// Raw text of the commit object.
    fn raw_text(&self) -> &[u8];
}
