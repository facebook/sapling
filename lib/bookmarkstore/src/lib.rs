// Copyright Facebook, Inc. 2018
//! bookmarkstore - Storage for bookmarks.
//!
//! The BookmarkStore provides and in-memory cache of bookmarks that are
//! persisted to a bookmark file once flush() is called.
//!
//! Bookmarks can be loaded from an existing hg bookmarks file.

#[macro_use]
extern crate failure;
extern crate indexedlog;
#[cfg(test)]
extern crate tempfile;
extern crate types;

use std::io::Write;
use std::path::Path;
use std::str;

use failure::Fallible;

use indexedlog::log::{IndexDef, IndexOutput, Log};
use types::node::Node;

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
        //   NODE_ID := fixed-length 20-byte node id
        //   BOOKMARK_NAME := variable-length bookmark name
        // On update or deletion, a new entry is appended.
        // * To lookup a bookmark, find the last entry with the bookmark.
        // * To lookup a node, find all entries with the node id. This gives a list of candidate
        //   bookmarks. For each candidate bookmark, lookup the bookmark (following the procedure
        //   of the previous bullet point) and check whether it is currently associated with
        //   the node.

        Ok(Self {
            log: Log::open(
                dir_path,
                vec![
                    IndexDef::new("bookmark", |data: &[u8]| match data[0] {
                        b'R' => vec![IndexOutput::Reference(1u64..data.len() as u64)],
                        b'U' => vec![IndexOutput::Reference(
                            (Node::len() + 1) as u64..data.len() as u64,
                        )],
                        c => panic!("invalid BookmarkEntry type '{}'", c),
                    }),
                    IndexDef::new("node", |data: &[u8]| match data[0] {
                        b'R' => vec![],
                        b'U' => vec![IndexOutput::Reference(1u64..(Node::len() + 1) as u64)],
                        c => panic!("invalid BookmarkEntry type '{}'", c),
                    }),
                ],
            )?,
        })
    }

    pub fn lookup_bookmark(&self, bookmark: &str) -> Option<Node> {
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
                    node,
                } => {
                    assert_eq!(found_bookmark, bookmark);
                    Some(node)
                }
            }
        })
    }

    pub fn lookup_node(&self, node: &Node) -> Option<Vec<String>> {
        let iter = self.log.lookup(1, &node).unwrap();
        let result = iter
            .filter_map(|data| {
                let data = data.unwrap();

                match BookmarkEntry::unpack(data) {
                    BookmarkEntry::Remove { bookmark: _ } => {
                        panic!("unreachable code");
                    }
                    BookmarkEntry::Update {
                        bookmark,
                        node: found_node,
                    } => {
                        assert_eq!(&found_node, node);
                        let latest_node = self.lookup_bookmark(bookmark);
                        match latest_node {
                            Some(latest_node) if &latest_node == node => {
                                Some(String::from(bookmark))
                            }
                            Some(_) => None, // bookmark still present, but points to another node
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

    pub fn update(&mut self, bookmark: &str, node: Node) -> Fallible<()> {
        Ok(self
            .log
            .append(BookmarkEntry::pack(&BookmarkEntry::Update {
                bookmark,
                node,
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
    Update { bookmark: &'a str, node: Node },
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
            BookmarkEntry::Update { bookmark, node } => {
                result.write_all(&['U' as u8]).unwrap();
                result.write_all(node.as_ref()).unwrap();
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
                let bookmark = str::from_utf8(&data[Node::len() + 1..]).unwrap();
                let node = Node::from_slice(&data[1..Node::len() + 1]).unwrap();
                BookmarkEntry::Update { bookmark, node }
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
        let node = Node::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(&bookmark, node).unwrap();
        assert_eq!(bm_store.lookup_bookmark(&bookmark).unwrap(), node);
        assert_eq!(
            bm_store.lookup_node(&node),
            Some(vec![bookmark.to_string()])
        );
    }

    #[test]
    fn test_multiple_bookmarks_for_single_node() {
        let bookmark = "test";
        let bookmark2 = "test2";
        let bookmark3 = "test3";
        let node = Node::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, node).unwrap();
        bm_store.update(bookmark2, node).unwrap();
        bm_store.update(bookmark3, node).unwrap();

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(node));
        assert_eq!(bm_store.lookup_bookmark(bookmark2), Some(node));
        assert_eq!(bm_store.lookup_bookmark(bookmark3), Some(node));
        let actual: HashSet<_> = HashSet::from_iter(bm_store.lookup_node(&node).unwrap());
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
        let node = Node::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, node).unwrap();
        bm_store.remove(bookmark).unwrap();
        assert_eq!(bm_store.lookup_bookmark(bookmark), None);
        assert_eq!(bm_store.lookup_node(&node), None);
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
        let node = Node::from_str("0123456789012345678901234567890123456789").unwrap();
        let node2 = Node::from(&[1u8; 20]);

        let (mut bm_store, _) = new_indexed_log_bookmark_store();

        bm_store.update(bookmark, node).unwrap();
        bm_store.update(bookmark, node2).unwrap();

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(node2));
    }

    #[test]
    fn test_write_bookmarks_to_file() {
        let bookmark = "testbookmark";
        let node = Node::from_str("0123456789012345678901234567890123456789").unwrap();

        let (mut original_bm_store, dir) = new_indexed_log_bookmark_store();
        original_bm_store.update(bookmark, node).unwrap();
        original_bm_store.flush().unwrap();

        let bm_store = BookmarkStore::new(dir.path()).unwrap();
        assert_eq!(
            bm_store.lookup_node(&node),
            Some(vec![String::from(bookmark)])
        );
        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(node));
    }
}
