/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkUpdateLogEntry;
pub fn group_entries(entries: Vec<BookmarkUpdateLogEntry>) -> Vec<BookmarkUpdateLogEntry> {
    let mut merged = vec![
        entries
            .first()
            .expect("BUL must have at least one entry")
            .clone(),
    ];
    for entry in &entries[1..] {
        let last_merged = merged.last_mut().unwrap();
        if entry.reason == last_merged.reason
            && entry.bookmark_name == last_merged.bookmark_name
            && (entry.from_changeset_id.is_none()
                || entry.from_changeset_id == last_merged.to_changeset_id)
        {
            last_merged.to_changeset_id = entry.to_changeset_id;
            last_merged.id = entry.id;
        } else {
            merged.push(entry.clone());
        }
    }
    merged
}
