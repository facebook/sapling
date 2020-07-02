/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The mutationstore is a store for recording records of commit mutations for
//! commits that are not in the local repository.
//!
//! It uses an indexedlog to store the data.  Each mutation entry corresponds to
//! the information of the mutation that led to the creation of a particular commit,
//! which is recorded as the successor in the entry.
//!
//! Entries can come from three possible places:
//!
//! * Commit metadata for a commit not available locally
//!
//! * Obsmarkers for repos that have been migrated from evolution tracking
//!
//! * Synthetic for entries created synthetically, e.g. by a pullcreatemarkers
//!   implementation.
//!
//! The other commits referred to in an entry must predate the successor commit.
//! For entries that originated from commits, this is ensured, as the successor
//! commit hash includes the other commit hashes.  For other entry types, it is
//! an error to refer to later commits, and any entry that causes a cycle will
//! be ignored.

use anyhow::Result;
use dag::namedag::MemNameDag;
use dag::ops::DagAddHeads;
use dag::VertexName;
use indexedlog::{
    log::{self as ilog, IndexDef, IndexOutput, Log},
    DefaultOpenOptions,
};
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use types::mutation::MutationEntry;
use types::node::Node;
use vlqencoding::VLQDecodeAt;

pub use indexedlog::Repair;

pub struct MutationStore {
    log: Log,
}

const INDEX_PRED: usize = 0;
const INDEX_SUCC: usize = 1;
const INDEX_SPLIT: usize = 2;

impl DefaultOpenOptions<ilog::OpenOptions> for MutationStore {
    fn default_open_options() -> ilog::OpenOptions {
        const NODE_LEN: usize = Node::len();
        const SUCC_START: usize = 1usize;
        const PRED_COUNT_START: usize = SUCC_START + NODE_LEN;
        let succ_index = |_data: &[u8]| {
            vec![IndexOutput::Reference(
                SUCC_START as u64..PRED_COUNT_START as u64,
            )]
        };
        let pred_index = |data: &[u8]| {
            let (pred_count, pred_start) = data
                .read_vlq_at(PRED_COUNT_START)
                .map(|(pred_count, vlq_size)| (pred_count, PRED_COUNT_START + vlq_size))
                .unwrap_or((0, 0));
            (0..pred_count)
                .map(|i| pred_start + NODE_LEN * i)
                .map(|i: usize| IndexOutput::Reference(i as u64..i as u64 + NODE_LEN as u64))
                .collect()
        };
        let split_index = |data: &[u8]| {
            let (split_count, split_start) = data
                .read_vlq_at(PRED_COUNT_START)
                .and_then(|(pred_count, vlq1_size): (usize, usize)| {
                    data.read_vlq_at(PRED_COUNT_START + vlq1_size + NODE_LEN * pred_count)
                        .map(|(split_count, vlq2_size)| {
                            (
                                split_count,
                                PRED_COUNT_START + vlq1_size + NODE_LEN * pred_count + vlq2_size,
                            )
                        })
                })
                .unwrap_or((0, 0));
            (0..split_count)
                .map(|i| split_start + NODE_LEN * i)
                .map(|i: usize| IndexOutput::Reference(i as u64..i as u64 + NODE_LEN as u64))
                .collect()
        };

        // Allow some lag to make the indexing more efficient.  Set to 10KB, which is roughly
        // 100 records.
        let lag_threshold = 10000;
        ilog::OpenOptions::new().create(true).index_defs(vec![
            IndexDef::new("pred", pred_index).lag_threshold(lag_threshold),
            IndexDef::new("succ", succ_index).lag_threshold(lag_threshold),
            IndexDef::new("split", split_index).lag_threshold(lag_threshold),
        ])
    }
}

impl MutationStore {
    pub fn open(path: impl AsRef<Path>) -> Result<MutationStore> {
        let log = Self::default_open_options().open(path.as_ref())?;
        Ok(MutationStore { log })
    }

    pub fn add(&mut self, entry: &MutationEntry) -> Result<()> {
        let mut buf = Vec::with_capacity(types::mutation::DEFAULT_ENTRY_SIZE);
        entry.serialize(&mut buf)?;
        self.log.append(buf.as_slice())?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.log.flush()?;
        Ok(())
    }

    pub fn get_successors_sets(&self, node: Node) -> Result<Vec<Vec<Node>>> {
        let mut successors_sets = Vec::new();
        for entry in self.log.lookup(INDEX_PRED, &node)? {
            let mutation_entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
            let mut successors = Vec::new();
            successors.extend(&mutation_entry.split);
            successors.push(mutation_entry.succ);
            successors_sets.push(successors);
        }
        Ok(successors_sets)
    }

    pub fn get_predecessors(&self, node: Node) -> Result<Vec<Node>> {
        let mut lookup = self
            .log
            .lookup(INDEX_SUCC, &node)?
            .chain(self.log.lookup(INDEX_SPLIT, &node)?);
        let predecessors = if let Some(entry) = lookup.next() {
            let mutation_entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
            mutation_entry.preds.clone()
        } else {
            vec![]
        };
        Ok(predecessors)
    }

    pub fn get_split_head(&self, node: Node) -> Result<Option<MutationEntry>> {
        let mutation_entry = match self.log.lookup(INDEX_SPLIT, &node)?.next() {
            Some(entry) => Some(MutationEntry::deserialize(&mut Cursor::new(entry?))?),
            None => None,
        };
        Ok(mutation_entry)
    }

    pub fn get(&self, succ: Node) -> Result<Option<MutationEntry>> {
        let mutation_entry = match self.log.lookup(INDEX_SUCC, &succ)?.next() {
            Some(entry) => Some(MutationEntry::deserialize(&mut Cursor::new(entry?))?),
            None => None,
        };
        Ok(mutation_entry)
    }

    /// Return a connected component that includes `nodes` and represents
    /// commit replacement relations.  The returned graph supports graph
    /// operations like common ancestors, heads, roots, etc. Parents in the
    /// graph are predecessors.
    pub fn get_dag(&self, nodes: Vec<Node>) -> Result<MemNameDag> {
        // Include successors recursively.
        let mut to_visit = nodes;
        let mut connected = HashSet::new();
        while let Some(node) = to_visit.pop() {
            if !connected.insert(node.clone()) {
                continue;
            }
            for entry in self.log.lookup(INDEX_PRED, &node)? {
                let entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
                to_visit.push(entry.succ);
            }
            for entry in self.log.lookup(INDEX_SUCC, &node)? {
                let entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
                for pred in entry.preds {
                    to_visit.push(pred);
                }
            }
        }
        let parent_func = |node| -> Result<Vec<VertexName>> {
            let mut result = Vec::new();
            for entry in self.log.lookup(INDEX_SUCC, &node)? {
                let entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
                for pred in entry.preds {
                    result.push(VertexName::copy_from(pred.as_ref()));
                }
            }
            Ok(result)
        };

        let mut dag = MemNameDag::new();
        let mut heads = connected
            .into_iter()
            .map(|s| VertexName::copy_from(s.as_ref()))
            .collect::<Vec<_>>();
        heads.sort();
        dag.add_heads(parent_func, &heads)?;
        Ok(dag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dag::DagAlgorithm;
    use rand::SeedableRng;
    use rand_chacha::ChaChaRng;
    use tempdir::TempDir;

    #[test]
    fn test_basic_store() {
        let mut rng = ChaChaRng::from_seed([0u8; 32]);
        let dir = TempDir::new("mutationstore").unwrap();
        let nodes = Node::random_distinct(&mut rng, 20);

        {
            let mut ms = MutationStore::open(dir.path()).expect("can open the store");
            ms.add(&MutationEntry {
                succ: nodes[1],
                preds: vec![nodes[0], nodes[2], nodes[3]],
                split: vec![],
                op: "fold".into(),
                user: "test".into(),
                time: 123456789,
                tz: -7200,
                extra: vec![(
                    Box::from(&b"note"[..]),
                    Box::from(&b"note about folding"[..]),
                )],
            })
            .expect("can add to the store");
            ms.add(&MutationEntry {
                succ: nodes[4],
                preds: vec![nodes[0]],
                split: vec![nodes[5], nodes[6]],
                op: "split".into(),
                user: "test".into(),
                time: 123456789,
                tz: -7200,
                extra: vec![],
            })
            .expect("can add to the store");

            ms.flush().expect("can flush the store");
        }
        {
            let ms = MutationStore::open(dir.path()).expect("can re-open the store");
            let mut expected_successors_sets =
                vec![vec![nodes[1]], vec![nodes[5], nodes[6], nodes[4]]];
            expected_successors_sets.sort_unstable();
            let mut successors_sets = ms
                .get_successors_sets(nodes[0])
                .expect("can get successors sets");
            successors_sets.sort_unstable();
            assert_eq!(successors_sets, expected_successors_sets);
            assert_eq!(
                ms.get_successors_sets(nodes[3])
                    .expect("can get successors sets"),
                vec![vec![nodes[1]]]
            );
            assert_eq!(
                ms.get_successors_sets(nodes[1])
                    .expect("can get successors sets"),
                Vec::<Vec<Node>>::new()
            );
            assert_eq!(
                ms.get_predecessors(nodes[1]).expect("can get predecessors"),
                vec![nodes[0], nodes[2], nodes[3]]
            );
            assert_eq!(
                ms.get_predecessors(nodes[4]).expect("can get predecessors"),
                vec![nodes[0]]
            );
            assert_eq!(
                ms.get_predecessors(nodes[5]).expect("can get predecessors"),
                vec![nodes[0]]
            );
            assert_eq!(
                ms.get_predecessors(nodes[3]).expect("can get predecessors"),
                vec![]
            );
            assert_eq!(
                ms.get_split_head(nodes[5])
                    .expect("can get split head")
                    .unwrap()
                    .succ,
                nodes[4],
            );
            assert_eq!(
                &ms.get(nodes[4])
                    .expect("can get mutation entry")
                    .unwrap()
                    .user,
                "test",
            );
            assert_eq!(
                &ms.get(nodes[1])
                    .expect("can get mutation entry")
                    .unwrap()
                    .extra[0]
                    .1,
                &Box::from(&b"note about folding"[..])
            );
        }
    }

    #[test]
    fn test_dag() -> Result<()> {
        let dir = TempDir::new("mutationstore")?;
        let mut ms = MutationStore::open(dir.path())?;
        let parents = drawdag::parse(
            r#"
             D E Z
             |\| |
             B C Y
             |/  |
             A   X
             "#,
        );
        // str (length 1) -> Node
        let n = |s: &str| -> Node { Node::from_slice(s.repeat(Node::len()).as_bytes()).unwrap() };
        let mut iter = parents.iter().collect::<Vec<_>>();
        iter.sort();
        for (name, parents) in iter {
            let node = n(name);
            for parent in parents {
                let parent = n(parent);
                ms.add(&MutationEntry {
                    succ: node,
                    preds: vec![parent],
                    split: vec![],
                    op: "rewrite".into(),
                    user: "test".into(),
                    time: 123456789,
                    tz: -7200,
                    extra: vec![],
                })?;
            }
        }

        let dag = ms.get_dag(vec![n("B")])?;
        assert_eq!(dag.all()?.count()?, 5); // A B C D E
        assert_eq!(
            renderdag::render_namedag(&dag, |v| Some(format!("({})", v.as_ref()[0] as char)))?,
            r#"
            o  4545454545454545454545454545454545454545 (E)
            │
            │ o  4444444444444444444444444444444444444444 (D)
            ╭─┤
            o │  4343434343434343434343434343434343434343 (C)
            │ │
            │ o  4242424242424242424242424242424242424242 (B)
            ├─╯
            o  4141414141414141414141414141414141414141 (A)"#
        );
        Ok(())
    }
}
