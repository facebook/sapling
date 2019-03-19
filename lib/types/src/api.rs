// Copyright Facebook, Inc. 2019.

//! Types for data interchange between the Mononoke API Server and the Mercurial client.

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};

use crate::{
    historyentry::{HistoryEntry, WireHistoryEntry},
    key::Key,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct FileDataResponse {
    files: Vec<(Key, Bytes)>,
}

impl FileDataResponse {
    pub fn new(data: impl IntoIterator<Item = (Key, Bytes)>) -> Self {
        Self {
            files: data.into_iter().collect(),
        }
    }
}

impl IntoIterator for FileDataResponse {
    type Item = (Key, Bytes);
    type IntoIter = std::vec::IntoIter<(Key, Bytes)>;

    fn into_iter(self) -> Self::IntoIter {
        self.files.into_iter()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileHistoryResponse {
    entries: Vec<(Vec<u8>, WireHistoryEntry)>,
}

impl FileHistoryResponse {
    pub fn new(history: impl IntoIterator<Item = (Vec<u8>, WireHistoryEntry)>) -> Self {
        Self {
            entries: history.into_iter().collect(),
        }
    }
}

impl IntoIterator for FileHistoryResponse {
    type Item = HistoryEntry;
    type IntoIter = Box<Iterator<Item = HistoryEntry> + Send + 'static>;

    fn into_iter(self) -> Self::IntoIter {
        let iter = self
            .entries
            .into_iter()
            .map(|(name, entry)| HistoryEntry::from_wire(entry, name));
        Box::new(iter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::{
        key::mocks::{BAR_KEY, BAZ_KEY, FOO_KEY},
        node::mocks::{AS, BS, CS, ONES, THREES, TWOS},
        nodeinfo::NodeInfo,
        parents::Parents,
    };

    #[test]
    fn data_iter() {
        let data = vec![
            (FOO_KEY.clone(), Bytes::from(&b"foo data"[..])),
            (BAR_KEY.clone(), Bytes::from(&b"bar data"[..])),
            (BAZ_KEY.clone(), Bytes::from(&b"baz data"[..])),
        ];
        let response = FileDataResponse::new(data.clone());

        let res = response.into_iter().collect::<Vec<_>>();
        assert_eq!(data, res);
    }

    #[test]
    fn history_iter() {
        let foo = WireHistoryEntry {
            node: ONES,
            parents: Parents::None,
            linknode: AS,
            copyfrom: None,
        };
        let bar = WireHistoryEntry {
            node: TWOS,
            parents: Parents::None,
            linknode: BS,
            copyfrom: None,
        };
        let baz = WireHistoryEntry {
            node: THREES,
            parents: Parents::None,
            linknode: CS,
            copyfrom: None,
        };

        let history = vec![
            (b"foo".to_vec(), foo),
            (b"bar".to_vec(), bar),
            (b"baz".to_vec(), baz),
        ];
        let response = FileHistoryResponse::new(history);

        let res = response.into_iter().collect::<Vec<_>>();
        let expected = vec![
            HistoryEntry {
                key: FOO_KEY.clone(),
                nodeinfo: NodeInfo {
                    linknode: AS,
                    ..Default::default()
                },
            },
            HistoryEntry {
                key: BAR_KEY.clone(),
                nodeinfo: NodeInfo {
                    linknode: BS,
                    ..Default::default()
                },
            },
            HistoryEntry {
                key: BAZ_KEY.clone(),
                nodeinfo: NodeInfo {
                    linknode: CS,
                    ..Default::default()
                },
            },
        ];

        assert_eq!(res, expected);
    }
}
