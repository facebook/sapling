// Copyright Facebook, Inc. 2019.

//! Types for data interchange between the Mononoke API Server and the Mercurial client.

use serde_derive::{Deserialize, Serialize};

use crate::{
    dataentry::DataEntry,
    historyentry::{HistoryEntry, WireHistoryEntry},
    key::Key,
    node::Node,
    path::RepoPathBuf,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct DataRequest {
    pub keys: Vec<Key>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DataResponse {
    pub entries: Vec<DataEntry>,
}

impl DataResponse {
    pub fn new(data: impl IntoIterator<Item = DataEntry>) -> Self {
        Self {
            entries: data.into_iter().collect(),
        }
    }
}

impl IntoIterator for DataResponse {
    type Item = DataEntry;
    type IntoIter = std::vec::IntoIter<DataEntry>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryRequest {
    pub keys: Vec<Key>,
    pub depth: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub entries: Vec<(RepoPathBuf, WireHistoryEntry)>,
}

impl HistoryResponse {
    pub fn new(history: impl IntoIterator<Item = (RepoPathBuf, WireHistoryEntry)>) -> Self {
        Self {
            entries: history.into_iter().collect(),
        }
    }
}

impl IntoIterator for HistoryResponse {
    type Item = HistoryEntry;
    type IntoIter = Box<Iterator<Item = HistoryEntry> + Send + 'static>;

    fn into_iter(self) -> Self::IntoIter {
        let iter = self
            .entries
            .into_iter()
            .map(|(path, entry)| HistoryEntry::from_wire(entry, path));
        Box::new(iter)
    }
}

/// Struct reprenting the arguments to a "gettreepack" operation, which
/// is used by Mercurial to prefetch treemanifests. This struct is intended
/// to provide a way to support requests compatible with Mercurial's existing
/// gettreepack wire protocol command.
///
/// In the future, we'd like to migrate away from requesting trees in this way.
/// In general, trees can be requested from the API server using a `DataRequest`
/// containing the keys of the desired tree nodes.
///
/// In all cases, trees will be returned in a `DataResponse`, so there is no
/// `TreeResponse` type to accompany `TreeRequest`.
#[derive(Debug, Serialize, Deserialize)]
pub struct TreeRequest {
    pub rootdir: RepoPathBuf,
    pub mfnodes: Vec<Node>,
    pub basemfnodes: Vec<Node>,
    pub depth: Option<usize>,
}

impl TreeRequest {
    pub fn new(
        rootdir: RepoPathBuf,
        mfnodes: Vec<Node>,
        basemfnodes: Vec<Node>,
        depth: Option<usize>,
    ) -> Self {
        Self {
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        }
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
        testutil::*,
    };

    #[test]
    fn data_iter() {
        let data = vec![
            data_entry(FOO_KEY.clone(), b"foo data"),
            data_entry(BAR_KEY.clone(), b"bar data"),
            data_entry(BAZ_KEY.clone(), b"baz data"),
        ];
        let response = DataResponse::new(data.clone());

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
            (repo_path_buf("foo"), foo),
            (repo_path_buf("bar"), bar),
            (repo_path_buf("baz"), baz),
        ];
        let response = HistoryResponse::new(history);

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
