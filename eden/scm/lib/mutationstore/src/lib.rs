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
use indexedlog::{
    log::{self as ilog, IndexDef, IndexOutput, Log},
    DefaultOpenOptions,
};
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
