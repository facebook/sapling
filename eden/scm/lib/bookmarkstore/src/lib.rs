/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! bookmarkstore - Storage for bookmarks.
//!
//! The BookmarkStore provides and in-memory cache of bookmarks that are
//! persisted to a bookmark file once flush() is called.
//!
//! Bookmarks can be loaded from an existing hg bookmarks file.

use std::io::Write;
use std::path::Path;
use std::str;

use failure::Fallible;

use indexedlog::log::{IndexDef, IndexOutput, Log};
use types::hgid::HgId;

pub mod errors;

pub struct BookmarkStore {
    log: Log,
}

impl BookmarkStore {
    pub fn new(dir_path: &Path) -> Fallible<Self> {
        // Log entry encoding:
        //   LOG := UPDATE | REMOVAL
        //   UPDATE := 'U' + NODE_ID + BOOKMARK_NAME
        //   REMOVAL := 'R' + BOOKMARK_NAME
        //   NODE_ID := fixed-length 20-byte hgid
        //   BOOKMARK_NAME := variable-length bookmark name
        // On update or deletion, a new entry is appended.
        // * To lookup a bookmark, find the last entry with the bookmark.
        // * To lookup a hgid, find all entries with the hgid. This gives a list of candidate
        //   bookmarks. For each candidate bookmark, lookup the bookmark (following the procedure
        //   of the previous bullet point) and check whether it is currently associated with
        //   the hgid.

        Ok(Self {
            log: Log::open(
                dir_path,
                vec![
                    IndexDef::new("bookmark", |data: &[u8]| match data[0] {
                        b'R' => vec![IndexOutput::Reference(1u64..data.len() as u64)],
                        b'U' => vec![IndexOutput::Reference(
                            (HgId::len() + 1) as u64..data.len() as u64,
                        )],
                        c => panic!("invalid BookmarkEntry type '{}'", c),
                    }),
                    IndexDef::new("node", |data: &[u8]| match data[0] {
                        b'R' => vec![],
                        b'U' => vec![IndexOutput::Reference(1u64..(HgId::len() + 1) as u64)],
                        c => panic!("invalid BookmarkEntry type '{}'", c),
                    }),
                ],
            )?,
        })
    }

    pub fn lookup_bookmark(&self, bookmark: &str) -> Option<HgId> {
        let mut iter = self.log.lookup(0, bookmark).unwrap();
        iter.next().and_then(|data| {
            let data = data.unwrap();
            match BookmarkEntry::unpack(data) {
                BookmarkEntry::Remove {
                    bookmark: found_bookmark,
                } => {
                    assert_eq!(found_bookmark, bookmark);
                    None
                }
                BookmarkEntry::Update {
                    bookmark: found_bookmark,
                    hgid,
                } => {
                    assert_eq!(found_bookmark, bookmark);
                    Some(hgid)
                }
            }
        })
    }

    pub fn lookup_hgid(&self, hgid: &HgId) -> Option<Vec<String>> {
        let iter = self.log.lookup(1, &hgid).unwrap();
        let result = iter
            .filter_map(|data| {
                let data = data.unwrap();

                match BookmarkEntry::unpack(data) {
                    BookmarkEntry::Remove { bookmark: _ } => {
                        panic!("unreachable code");
                    }
                    BookmarkEntry::Update {
                        bookmark,
                        hgid: found_hgid,
                    } => {
                        assert_eq!(&found_hgid, hgid);
                        let latest_hgid = self.lookup_bookmark(bookmark);
                        match latest_hgid {
                            Some(latest_hgid) if &latest_hgid == hgid => {
                                Some(String::from(bookmark))
                            }
                            Some(_) => None, // bookmark still present, but points to another hgid
                            None => None,    // bookmark has been removed
                        }
                    }
                }
            })
            .collect::<Vec<_>>();
        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    pub fn update(&mut self, bookmark: &str, hgid: HgId) -> Fallible<()> {
        Ok(self
            .log
            .append(BookmarkEntry::pack(&BookmarkEntry::Update {
                bookmark,
                hgid,
            }))?)
    }

    pub fn remove(&mut self, bookmark: &str) -> Fallible<()> {
        if self.lookup_bookmark(bookmark).is_none() {
            return Err(errors::BookmarkNotFound {
                name: bookmark.to_string(),
            }
            .into());
        }
        Ok(self
            .log
            .append(BookmarkEntry::pack(&BookmarkEntry::Remove { bookmark }))?)
    }

    pub fn flush(&mut self) -> Fallible<()> {
        self.log.flush()?;
        Ok(())
    }
}

enum BookmarkEntry<'a> {
    Update { bookmark: &'a str, hgid: HgId },
    Remove { bookmark: &'a str },
}

impl<'a> BookmarkEntry<'a> {
    fn pack(bookmark_entry: &BookmarkEntry<'_>) -> Vec<u8> {
        let mut result = Vec::new();
        match bookmark_entry {
            BookmarkEntry::Remove { bookmark } => {
                result.write_all(&['R' as u8]).unwrap();
                result.write_all(bookmark.as_bytes()).unwrap();
            }
            BookmarkEntry::Update { bookmark, hgid } => {
                result.write_all(&['U' as u8]).unwrap();
                result.write_all(hgid.as_ref()).unwrap();
                result.write_all(bookmark.as_bytes()).unwrap();
            }
        }
        result
    }

    fn unpack(data: &[u8]) -> BookmarkEntry<'_> {
        match data[0] {
            b'R' => {
                let bookmark = str::from_utf8(&data[1..]).unwrap();
                BookmarkEntry::Remove { bookmark }
            }
            b'U' => {
                let bookmark = str::from_utf8(&data[HgId::len() + 1..]).unwrap();
                let hgid = HgId::from_slice(&data[1..HgId::len() + 1]).unwrap();
                BookmarkEntry::Update { bookmark, hgid }
            }
            c => panic!("invalid BookmarkEntry type '{}'", c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::iter::FromIterator;
    use tempfile::TempDir;

    fn new_indexed_log_bookmark_store() -> (BookmarkStore, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let bm_store = BookmarkStore::new(dir.path()).unwrap();
        (bm_store, dir)
    }

    #[test]
    fn test_update() {
        let bookmark = "test";
        let hgid = HgId::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(&bookmark, hgid).unwrap();
        assert_eq!(bm_store.lookup_bookmark(&bookmark).unwrap(), hgid);
        assert_eq!(
            bm_store.lookup_hgid(&hgid),
            Some(vec![bookmark.to_string()])
        );
    }

    #[test]
    fn test_multiple_bookmarks_for_single_hgid() {
        let bookmark = "test";
        let bookmark2 = "test2";
        let bookmark3 = "test3";
        let hgid = HgId::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, hgid).unwrap();
        bm_store.update(bookmark2, hgid).unwrap();
        bm_store.update(bookmark3, hgid).unwrap();

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(hgid));
        assert_eq!(bm_store.lookup_bookmark(bookmark2), Some(hgid));
        assert_eq!(bm_store.lookup_bookmark(bookmark3), Some(hgid));
        let actual: HashSet<_> = HashSet::from_iter(bm_store.lookup_hgid(&hgid).unwrap());
        let expected = HashSet::from_iter(vec![
            String::from(bookmark),
            String::from(bookmark2),
            String::from(bookmark3),
        ]);
        assert_eq!(actual, expected);
    }

    #[test]
    fn test_remove() {
        let bookmark = "test";
        let hgid = HgId::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, hgid).unwrap();
        bm_store.remove(bookmark).unwrap();
        assert_eq!(bm_store.lookup_bookmark(bookmark), None);
        assert_eq!(bm_store.lookup_hgid(&hgid), None);
    }

    #[test]
    fn test_remove_non_existent_bookmark() {
        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        let ret = bm_store.remove("missing");
        assert_eq!(
            format!("{}", ret.unwrap_err()),
            "bookmark not found: missing"
        );
    }

    #[test]
    fn test_update_bookmark() {
        let bookmark = "test";
        let hgid = HgId::from_str("0123456789012345678901234567890123456789").unwrap();
        let node2 = HgId::from(&[1u8; 20]);

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, hgid).unwrap();
        bm_store.update(bookmark, node2).unwrap();

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(node2));
    }

    #[test]
    fn test_write_bookmarks_to_file() {
        let bookmark = "testbookmark";
        let hgid = HgId::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut original_bm_store, dir) = new_indexed_log_bookmark_store();
        original_bm_store.update(bookmark, hgid).unwrap();
        original_bm_store.flush().unwrap();

        let bm_store = BookmarkStore::new(dir.path()).unwrap();
        assert_eq!(
            bm_store.lookup_hgid(&hgid),
            Some(vec![String::from(bookmark)])
        );
        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(hgid));
    }
}
