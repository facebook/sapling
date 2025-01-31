/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkKey;
use megarepolib::common::ChangesetArgs as MegarepoNewChangesetArgs;
use mononoke_types::DateTime;

#[derive(Debug, clap::Args, Clone)]
pub(crate) struct ResultingChangesetArgs {
    #[clap(long, short = 'm')]
    pub commit_message: String,
    #[clap(long, short = 'a')]
    pub commit_author: String,

    #[clap(long = "commit-date-rfc3339")]
    pub datetime: Option<String>,

    #[clap(
        long,
        help = "bookmark to point to resulting commits (no sanity checks, will move existing bookmark, be careful)"
    )]
    pub set_bookmark: Option<String>,

    #[clap(long = "mark-public")]
    pub mark_public: bool,
}

impl TryInto<MegarepoNewChangesetArgs> for ResultingChangesetArgs {
    type Error = Error;

    fn try_into(self) -> Result<MegarepoNewChangesetArgs> {
        let mb_datetime = self
            .datetime
            .as_deref()
            .map_or_else(|| Ok(DateTime::now()), DateTime::from_rfc3339)?;

        let mb_bookmark = self.set_bookmark.map(BookmarkKey::new).transpose()?;
        let res = MegarepoNewChangesetArgs {
            message: self.commit_message,
            author: self.commit_author,
            datetime: mb_datetime,
            bookmark: mb_bookmark,
            mark_public: self.mark_public,
        };
        Ok(res)
    }
}
