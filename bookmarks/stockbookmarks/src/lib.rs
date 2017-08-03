// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
#[macro_use]
#[cfg(test)]
extern crate assert_matches;
#[macro_use]
extern crate error_chain;
extern crate futures;

extern crate bookmarks;
extern crate mercurial_types;

mod errors {
    error_chain! {
        errors {
            InvalidBookmarkLine(line: Vec<u8>) {
                description("invalid bookmark line")
                display("invalid bookmark line: {}", String::from_utf8_lossy(line))
            }
            InvalidHash(hex: Vec<u8>) {
                description("invalid hash")
                display("invalid hash: {}", String::from_utf8_lossy(hex))
            }
        }
        foreign_links {
            Io(::std::io::Error);
        }
    }
}

use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use std::marker::PhantomData;
use std::error;

use ascii::AsciiStr;
use futures::future::{self, FutureResult};
use futures::stream::{self, BoxStream, Stream};

use bookmarks::{Bookmarks, Version};
use mercurial_types::NodeHash;

pub use errors::*;

/// Implementation of bookmarks as they exist in stock Mercurial inside `.hg/bookmarks`.
/// The file has a list of entries:
///
/// ```
/// <hash1> <bookmark1-name>
/// <hash2> <bookmark2-name>
/// ...
/// ```
///
/// Bookmark names are arbitrary bytestrings, and hashes are always NodeHashes.
///
/// This implementation is read-only -- implementing write support would require interacting with
/// the locking mechanism Mercurial uses, and generally seems like it wouldn't be very useful.
#[derive(Debug)]
pub struct StockBookmarks<E = Error> {
    bookmarks: HashMap<Vec<u8>, NodeHash>,
    _phantom: PhantomData<E>,
}

impl<E> StockBookmarks<E>
where
    E: From<Error> + Send + error::Error,
{
    pub fn read<P: Into<PathBuf>>(base: P) -> Result<Self> {
        let base = base.into();

        let file = fs::File::open(base.join("bookmarks"));
        match file {
            Ok(file) => Self::from_reader(file),
            Err(ref err) if err.kind() == io::ErrorKind::NotFound => {
                // The .hg/bookmarks file is not guaranteed to exist. Treat it is empty if it
                // doesn't.
                Ok(StockBookmarks {
                    bookmarks: HashMap::new(),
                    _phantom: PhantomData,
                })
            }
            Err(err) => Err(err.into()),
        }
    }

    fn from_reader<R: Read>(reader: R) -> Result<Self> {
        let mut bookmarks = HashMap::new();

        // Bookmark names might not be valid UTF-8, so use split() instead of lines().
        for line in BufReader::new(reader).split(b'\n') {
            let line = line?;
            // <hash><space><bookmark name>, where hash is 40 bytes, the space is 1 byte
            // and the bookmark name is at least 1 byte.
            if line.len() < 42 || line[40] != b' ' {
                bail!(ErrorKind::InvalidBookmarkLine(line));
            }
            let bmname = &line[41..];
            let hash_slice = &line[..40];
            let hash = AsciiStr::from_ascii(&hash_slice)
                .chain_err(|| ErrorKind::InvalidHash(hash_slice.into()))?;
            bookmarks.insert(
                bmname.into(),
                NodeHash::from_ascii_str(hash)
                    .chain_err(|| ErrorKind::InvalidHash(hash_slice.into()))?,
            );
        }

        Ok(StockBookmarks {
            bookmarks,
            _phantom: PhantomData,
        })
    }
}

impl<E> Bookmarks for StockBookmarks<E>
where
    E: From<Error> + error::Error + Send + 'static,
{
    type Value = NodeHash;
    type Error = E;

    type Get = FutureResult<Option<(NodeHash, Version)>, E>;
    type Keys = BoxStream<Vec<u8>, E>;

    fn get(&self, name: &AsRef<[u8]>) -> Self::Get {
        let value = match self.bookmarks.get(name.as_ref()) {
            Some(hash) => Some((*hash, Version::from(1))),
            None => None,
        };
        future::result(Ok(value))
    }

    fn keys(&self) -> Self::Keys {
        // collect forces evaluation early, so that the stream can safely outlive self
        stream::iter(
            self.bookmarks
                .keys()
                .map(|k| Ok(k.to_vec()))
                .collect::<Vec<_>>(),
        ).boxed()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use futures::Future;

    use super::*;

    fn hash_ones() -> NodeHash {
        "1111111111111111111111111111111111111111".parse().unwrap()
    }

    fn hash_twos() -> NodeHash {
        "2222222222222222222222222222222222222222".parse().unwrap()
    }

    fn assert_bookmark_get(
        bookmarks: &StockBookmarks,
        key: &AsRef<[u8]>,
        expected: Option<NodeHash>,
    ) {
        let expected = match expected {
            Some(hash) => Some((hash, Version::from(1))),
            None => None,
        };
        assert_eq!(bookmarks.get(key).wait().unwrap(), expected);
    }

    #[test]
    fn test_parse() {
        let disk_bookmarks = b"\
            1111111111111111111111111111111111111111 abc\n\
            2222222222222222222222222222222222222222 def\n\
            1111111111111111111111111111111111111111 test123\n";
        let reader = Cursor::new(&disk_bookmarks[..]);

        let bookmarks = StockBookmarks::from_reader(reader).unwrap();
        assert_bookmark_get(&bookmarks, &"abc", Some(hash_ones()));
        assert_bookmark_get(&bookmarks, &"def", Some(hash_twos()));
        assert_bookmark_get(&bookmarks, &"test123", Some(hash_ones()));

        // Bookmarks that aren't present
        assert_bookmark_get(&bookmarks, &"abcdef", None);

        // keys should return all the keys here
        let mut list = bookmarks.keys().collect().wait().unwrap();
        list.sort();
        assert_eq!(list, vec![&b"abc"[..], &b"def"[..], &b"test123"[..]]);
    }

    /// Test a bunch of invalid bookmark lines
    #[test]
    fn test_invalid() {
        let reader = Cursor::new(&b"111\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidBookmarkLine(_));

        // no space or bookmark name
        let reader = Cursor::new(&b"1111111111111111111111111111111111111111\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidBookmarkLine(_));

        // no bookmark name
        let reader = Cursor::new(&b"1111111111111111111111111111111111111111 \n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidBookmarkLine(_));

        // no space after hash
        let reader = Cursor::new(&b"1111111111111111111111111111111111111111ab\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidBookmarkLine(_));

        // short hash
        let reader = Cursor::new(&b"111111111111111111111111111111111111111  1ab\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidHash(_));

        // non-ASCII
        let reader = Cursor::new(&b"111111111111111111111111111111111111111\xff test\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidHash(_));

        // not a valid hex string
        let reader = Cursor::new(&b"abcdefgabcdefgabcdefgabcdefgabcdefgabcde test\n"[..]);
        let bookmarks = StockBookmarks::<Error>::from_reader(reader);
        assert_matches!(bookmarks.unwrap_err().kind(), &ErrorKind::InvalidHash(_));
    }
}
