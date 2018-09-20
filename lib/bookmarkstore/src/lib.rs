// Copyright Facebook, Inc. 2018
//! bookmarkstore - Storage for bookmarks.
//!
//! The BookmarkStore provides and in-memory cache of bookmarks that are
//! persisted to a bookmark file once flush() is called.
//!
//! Bookmarks can be loaded from an existing hg bookmarks file.

extern crate atomicwrites;
#[macro_use]
extern crate error_chain;
extern crate revisionstore;
#[cfg(test)]
extern crate tempfile;

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

use atomicwrites::{AllowOverwrite, AtomicFile};
use revisionstore::node::Node;

pub mod errors;
pub use errors::{Error, ErrorKind, Result};

#[derive(Clone, Debug)]
pub struct BookmarkStore {
    bookmarks: HashMap<String, Node>,
    nodes: HashMap<Node, Vec<String>>,
}

impl BookmarkStore {
    pub fn new() -> Self {
        Self {
            bookmarks: HashMap::new(),
            nodes: HashMap::new(),
        }
    }

    pub fn from_file(file_path: &Path) -> Result<Self> {
        let mut bs = Self::new();
        bs.load_bookmarks(file_path)?;
        Ok(bs)
    }

    fn load_bookmarks(&mut self, file_path: &Path) -> Result<()> {
        let mut bookmark_file = File::open(file_path)?;
        let mut file_contents = String::new();
        let mut line_num = 0;

        bookmark_file.read_to_string(&mut file_contents)?;

        for line in file_contents.lines() {
            let line_chunks: Vec<_> = line.splitn(2, ' ').collect();
            line_num += 1;

            if line_chunks.len() == 2 {
                let bookmark = line_chunks[1].to_string();

                match Node::from_str(line_chunks[0]) {
                    Ok(node) => {
                        self.bookmarks.insert(bookmark.clone(), node);
                        self.nodes
                            .entry(node)
                            .and_modify(|v| v.push(bookmark.clone()))
                            .or_insert_with(|| vec![bookmark.clone()]);
                    }
                    Err(_) => {
                        bail!(ErrorKind::MalformedBookmarkFile(line_num, line.to_string()));
                    }
                }
            } else {
                bail!(ErrorKind::MalformedBookmarkFile(line_num, line.to_string()));
            }
        }

        Ok(())
    }

    pub fn lookup_bookmark<S: AsRef<str>>(&self, bookmark: S) -> Option<&Node> {
        self.bookmarks.get(bookmark.as_ref())
    }

    pub fn lookup_node(&self, node: Node) -> Option<&Vec<String>> {
        self.nodes.get(&node)
    }

    pub fn add_bookmark<S: AsRef<str>>(&mut self, bookmark: S, node: Node) {
        let bookmark = bookmark.as_ref();

        if let Some(node) = self.bookmarks.get(bookmark).cloned() {
            self.remove_node_to_bookmark_mapping(bookmark, &node);
        };

        self.bookmarks.insert(bookmark.to_string(), node);
        self.nodes
            .entry(node)
            .and_modify(|v| v.push(bookmark.to_string()))
            .or_insert_with(|| vec![bookmark.to_string()]);
    }

    fn remove_node_to_bookmark_mapping<S: AsRef<str>>(&mut self, bookmark: S, node: &Node) {
        let num_bookmarks = {
            let bm_vec = self.nodes.get_mut(node).unwrap();
            bm_vec.retain(|b| b != bookmark.as_ref());
            bm_vec.len()
        };

        if num_bookmarks == 0 {
            self.nodes.remove(node);
        }
    }

    pub fn remove_bookmark<S: AsRef<str>>(&mut self, bookmark: S) -> Result<()> {
        let node = match self.lookup_bookmark(bookmark.as_ref()) {
            Some(node) => node.clone(),
            None => bail!(ErrorKind::BookmarkNotFound(bookmark.as_ref().to_string())),
        };

        self.bookmarks.remove(bookmark.as_ref());
        self.remove_node_to_bookmark_mapping(bookmark.as_ref(), &node);

        Ok(())
    }

    pub fn flush(&mut self, file_path: &Path) -> Result<()> {
        let lines: Vec<_> = self.bookmarks
            .iter()
            .map(|(b, n)| format!("{} {}", n, b))
            .collect();

        AtomicFile::new(file_path, AllowOverwrite)
            .write(|f| f.write_all(lines.join("\n").as_bytes()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use revisionstore::node::Node;
    use tempfile::NamedTempFile;

    #[test]
    fn test_add_bookmark() {
        let bookmark = "test";
        let node = Node::default();

        let mut bm_store = BookmarkStore::new();

        bm_store.add_bookmark(&bookmark, node);
        assert_eq!(bm_store.lookup_bookmark(&bookmark).unwrap(), &node);
        assert_eq!(
            bm_store.lookup_node(node),
            Some(&vec![bookmark.to_string()])
        );
    }

    #[test]
    fn test_multiple_bookmarks_for_single_node() {
        let bookmark = "test";
        let bookmark2 = "test2";
        let bookmark3 = "test3";
        let node = Node::default();

        let mut bm_store = BookmarkStore::new();

        bm_store.add_bookmark(bookmark, node);
        bm_store.add_bookmark(bookmark2, node);
        bm_store.add_bookmark(bookmark3, node);

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(&node));
        assert_eq!(bm_store.lookup_bookmark(bookmark2), Some(&node));
        assert_eq!(bm_store.lookup_bookmark(bookmark3), Some(&node));
        assert_eq!(bm_store.lookup_node(node).unwrap().len(), 3);
    }

    #[test]
    fn test_remove_bookmark() {
        let bookmark = "test";
        let node = Node::default();

        let mut bm_store = BookmarkStore::new();

        bm_store.add_bookmark(bookmark, node);
        bm_store.remove_bookmark(bookmark).unwrap();
        assert_eq!(bm_store.lookup_bookmark(bookmark), None);
        assert_eq!(bm_store.lookup_node(node), None);
    }

    #[test]
    fn test_remove_non_existent_bookmark() {
        let mut bm_store = BookmarkStore::new();

        let ret = bm_store.remove_bookmark("missing");
        assert_eq!(
            format!("{}", ret.unwrap_err()),
            "bookmark not found: missing"
        );
    }

    #[test]
    fn test_update_bookmark() {
        let bookmark = "test";
        let node = Node::default();
        let node2 = Node::from(&[1u8; 20]);

        let mut bm_store = BookmarkStore::new();

        bm_store.add_bookmark(bookmark, node);
        bm_store.add_bookmark(bookmark, node2);

        assert_eq!(bm_store.lookup_bookmark(bookmark), Some(&node2));
    }

    #[test]
    fn test_load_malformed_bookmark_file() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"malformed test-bookmark malformed")
            .unwrap();
        let path = file.path();

        let bm_store = BookmarkStore::from_file(path);
        assert_eq!(
            format!("{}", bm_store.unwrap_err()),
            "malformed bookmark file at line 1: malformed test-bookmark malformed"
        );
    }

    #[test]
    fn test_load_bookmarks_from_file() {
        let mut file = NamedTempFile::new().unwrap();
        let bm_str_prefix = "123456781234567812345678123456781234";
        let mut contents = String::new();
        contents.push_str(&format!("{}0000 test-bookmark\n", bm_str_prefix));
        contents.push_str(&format!("{}0000 test-dupl\n", bm_str_prefix));
        contents.push_str(&format!("{}1111 test bookmark spaces\n", bm_str_prefix));

        file.write_all(contents.as_bytes()).unwrap();
        let path = file.path();

        let bm_store = BookmarkStore::from_file(path).unwrap();
        assert_eq!(
            bm_store.lookup_bookmark("test-dupl"),
            Some(&Node::from_str(&format!("{}0000", bm_str_prefix)).unwrap())
        );
        assert_eq!(
            bm_store.lookup_bookmark("test-bookmark"),
            Some(&Node::from_str(&format!("{}0000", bm_str_prefix)).unwrap())
        );
        assert_eq!(
            bm_store.lookup_bookmark("test bookmark spaces"),
            Some(&Node::from_str(&format!("{}1111", bm_str_prefix)).unwrap())
        );
    }

    #[test]
    fn test_write_bookmarks_to_file() {
        let file = NamedTempFile::new().unwrap();
        let bookmark = "test";
        let node = Node::default();
        let mut s = String::new();
        let output = "0000000000000000000000000000000000000000 test";

        let mut bm_store = BookmarkStore::from_file(file.path()).unwrap();
        bm_store.add_bookmark(bookmark, node);

        bm_store.flush(file.path()).unwrap();
        // As the file is replaced atomically, we need to open a new handle to the file.
        File::open(file.path())
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();

        assert_eq!(s, output);
    }
}
