/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkUpdateLogEntry;

pub(crate) struct GroupedBookmarkUpdateLogEntry {
    pub(crate) upload_entry: BookmarkUpdateLogEntry,
    pub(crate) bookmark_entries: Vec<BookmarkUpdateLogEntry>,
}

impl GroupedBookmarkUpdateLogEntry {
    pub(crate) fn single(entry: BookmarkUpdateLogEntry) -> Self {
        Self {
            upload_entry: entry.clone(),
            bookmark_entries: vec![entry],
        }
    }
}

pub fn group_entries(entries: Vec<BookmarkUpdateLogEntry>) -> Vec<GroupedBookmarkUpdateLogEntry> {
    let mut entries = entries.into_iter();
    let first = entries.next().expect("BUL must have at least one entry");
    let mut grouped = vec![GroupedBookmarkUpdateLogEntry::single(first)];

    for entry in entries {
        let last_group = grouped
            .last_mut()
            .expect("grouped BUL must have at least one entry");
        let last_merged = &mut last_group.upload_entry;
        if entry.reason == last_merged.reason
            && entry.bookmark_name == last_merged.bookmark_name
            && (entry.from_changeset_id.is_none()
                || entry.from_changeset_id == last_merged.to_changeset_id)
        {
            last_merged.to_changeset_id = entry.to_changeset_id;
            last_merged.id = entry.id;
            last_group.bookmark_entries.push(entry);
        } else {
            grouped.push(GroupedBookmarkUpdateLogEntry::single(entry));
        }
    }

    grouped
}
