/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::fmt::Debug;
use std::mem;

use bytes::Bytes;
use failure::{Error, Fail};
use failure_ext::Result;
use futures::{Poll, Stream};

use mercurial_bundles::wirepack::converter::{WirePackConverter, WirePackPartProcessor};
use mercurial_bundles::wirepack::{DataEntry, HistoryEntry, Part};
use mercurial_revlog::manifest::ManifestContent;
use mercurial_types::{delta, HgNodeHash, HgNodeKey, RepoPath, NULL_HASH};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Malformed treemanifest part: {}", _0)]
    MalformedTreemanifestPart(String),
}

/// Parser for wirepack tree part. It returns a stream of TreemanifestEntry, that can be used by
/// Mononoke's Commit Api.
///
/// It assumes a few things:
/// 1) all data is sent as a delta from the null revision (i.e. data is basically non-deltaed).
/// 2) there are exactly one history entry and exactly one data entry for each tree.
pub struct TreemanifestBundle2Parser<S> {
    stream: WirePackConverter<S, TreemanifestPartProcessor>,
}

impl<S> TreemanifestBundle2Parser<S>
where
    S: Stream<Item = Part, Error = Error>,
{
    pub fn new(part_stream: S) -> Self {
        Self {
            stream: WirePackConverter::new(part_stream, TreemanifestPartProcessor::new()),
        }
    }
}

impl<S> Stream for TreemanifestBundle2Parser<S>
where
    S: Stream<Item = Part, Error = Error>,
{
    type Item = TreemanifestEntry;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Error> {
        self.stream.poll()
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct TreemanifestEntry {
    pub node_key: HgNodeKey,
    pub data: Bytes,
    pub p1: Option<HgNodeHash>,
    pub p2: Option<HgNodeHash>,
    pub manifest_content: ManifestContent,
}

impl TreemanifestEntry {
    fn new(node_key: HgNodeKey, data: Bytes, p1: HgNodeHash, p2: HgNodeHash) -> Result<Self> {
        let manifest_content = ManifestContent::parse(data.as_ref())?;

        Ok(Self {
            node_key,
            data,
            p1: p1.into_option(),
            p2: p2.into_option(),
            manifest_content,
        })
    }
}

struct TreemanifestPartProcessor {
    node: Option<HgNodeHash>,
    p1: Option<HgNodeHash>,
    p2: Option<HgNodeHash>,
    path: Option<RepoPath>,
}

impl TreemanifestPartProcessor {
    fn new() -> Self {
        Self {
            node: None,
            p1: None,
            p2: None,
            path: None,
        }
    }
}

impl WirePackPartProcessor for TreemanifestPartProcessor {
    type Data = TreemanifestEntry;

    fn history_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>> {
        replace_or_fail_if_exists(&mut self.path, path.clone())?;
        if entry_count != 1 {
            let msg = format!("expected exactly one history entry, got: {}", entry_count);
            return Err(ErrorKind::MalformedTreemanifestPart(msg).into());
        }
        Ok(None)
    }

    fn history(&mut self, entry: &HistoryEntry) -> Result<Option<Self::Data>> {
        replace_or_fail_if_exists(&mut self.node, entry.node.clone())?;
        replace_or_fail_if_exists(&mut self.p1, entry.p1.clone())?;
        replace_or_fail_if_exists(&mut self.p2, entry.p2.clone())?;
        Ok(None)
    }

    fn data_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>> {
        if Some(path) != self.path.as_ref() {
            let msg = format!("unexpected path: {:?} != {:?}", path, self.path);
            Err(ErrorKind::MalformedTreemanifestPart(msg).into())
        } else if entry_count != 1 {
            let msg = format!("expected exactly one data entry, got: {}", entry_count);
            Err(ErrorKind::MalformedTreemanifestPart(msg).into())
        } else {
            Ok(None)
        }
    }

    fn data(&mut self, data_entry: &DataEntry) -> Result<Option<Self::Data>> {
        if data_entry.delta_base != NULL_HASH {
            let msg = format!("unexpected delta base: {:?}", data_entry.delta_base);
            return Err(ErrorKind::MalformedTreemanifestPart(msg).into());
        }

        let node_key = HgNodeKey {
            path: unwrap_field(&mut self.path, "path")?,
            hash: unwrap_field(&mut self.node, "node")?,
        };
        let bytes = Bytes::from(delta::apply("".as_bytes(), &data_entry.delta)?);
        let p1 = unwrap_field(&mut self.p1, "p1")?;
        let p2 = unwrap_field(&mut self.p2, "p2")?;

        Ok(Some(TreemanifestEntry::new(node_key, bytes, p1, p2)?))
    }

    fn end(&mut self) -> Result<Option<Self::Data>> {
        Ok(None)
    }
}

fn replace_or_fail_if_exists<T: Debug>(existing: &mut Option<T>, new_value: T) -> Result<()> {
    let existing = mem::replace(existing, Some(new_value));
    if !existing.is_none() {
        let msg = format!("{:?} was already set", existing);
        Err(ErrorKind::MalformedTreemanifestPart(msg).into())
    } else {
        Ok(())
    }
}

fn unwrap_field<T: Clone>(field: &mut Option<T>, field_name: &str) -> Result<T> {
    field.take().ok_or_else(|| {
        let msg = format!("{} is not set", field_name);
        ErrorKind::MalformedTreemanifestPart(msg).into()
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::{stream, Future};

    use maplit::btreemap;
    use mercurial_revlog::manifest::Details;
    use mercurial_types::manifest::Type;
    use mercurial_types::{FileType, MPath};
    use mercurial_types_mocks::nodehash::*;

    #[test]
    fn test_simple() {
        let parts = vec![
            get_history_meta(),
            get_history_entry(),
            get_data_meta(),
            get_data_entry(),
            get_history_meta(),
            get_history_entry(),
            get_data_meta(),
            get_data_entry(),
            Part::End,
        ];

        let part_stream = stream::iter_ok(parts.into_iter());
        let stream = TreemanifestBundle2Parser::new(part_stream);
        assert_eq!(
            stream.collect().wait().unwrap(),
            vec![get_expected_entry(), get_expected_entry()]
        );
    }

    #[test]
    fn test_broken() {
        let parts = vec![get_history_meta(), get_history_entry(), Part::End];
        assert_fails(parts);
        let parts = vec![
            get_history_meta(),
            get_history_entry(),
            get_data_meta(),
            Part::End,
        ];
        assert_fails(parts);
        let parts = vec![
            get_history_meta(),
            get_history_entry(),
            get_data_entry(),
            get_data_meta(),
            Part::End,
        ];
        assert_fails(parts);

        let parts = vec![
            get_history_meta(),
            get_history_entry(),
            Part::DataMeta {
                path: RepoPath::dir("dir").unwrap(),
                entry_count: 1,
            },
            get_data_entry(),
            Part::End,
        ];
        assert_fails(parts);
    }

    fn get_history_meta() -> Part {
        Part::HistoryMeta {
            path: RepoPath::root(),
            entry_count: 1,
        }
    }

    fn get_history_entry() -> Part {
        let node = ONES_HASH;
        let p1 = TWOS_HASH;
        let p2 = THREES_HASH;
        let linknode = FOURS_HASH;

        Part::History(HistoryEntry {
            node,
            p1,
            p2,
            linknode,
            copy_from: None,
        })
    }

    fn get_data_meta() -> Part {
        Part::DataMeta {
            path: RepoPath::root(),
            entry_count: 1,
        }
    }

    fn get_revlog_manifest_content() -> ManifestContent {
        ManifestContent {
            files: btreemap! {
                MPath::new("test_dir/test_file").unwrap() =>
                Details::new(
                    ONES_HASH,
                    Type::File(FileType::Regular),
                ),
                MPath::new("test_dir2/test_manifest").unwrap() =>
                Details::new(
                    TWOS_HASH,
                    Type::Tree,
                ),
            },
        }
    }

    fn get_data_entry() -> Part {
        let node = ONES_HASH;

        let data = {
            let mut data = Vec::new();
            get_revlog_manifest_content().generate(&mut data).unwrap();
            data
        };

        Part::Data(DataEntry {
            node,
            delta_base: NULL_HASH,
            delta: delta::Delta::new_fulltext(data),
            metadata: None,
        })
    }

    fn assert_fails(parts: Vec<Part>) {
        let part_stream = stream::iter_ok(parts.into_iter());
        let stream = TreemanifestBundle2Parser::new(part_stream);
        assert!(stream.collect().wait().is_err());
    }

    fn get_expected_entry() -> TreemanifestEntry {
        let node_key = HgNodeKey {
            path: RepoPath::root(),
            hash: ONES_HASH,
        };
        let p1 = TWOS_HASH;
        let p2 = THREES_HASH;

        let data = {
            let mut data = Vec::new();
            get_revlog_manifest_content().generate(&mut data).unwrap();
            data
        };

        let entry = TreemanifestEntry::new(node_key, Bytes::from(data), p1, p2).unwrap();

        assert_eq!(
            entry.manifest_content,
            get_revlog_manifest_content(),
            "Sanity check for manifest content failed"
        );

        entry
    }
}
