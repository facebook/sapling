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

#![allow(clippy::redundant_closure)]

use anyhow::Result;
use bitflags::bitflags;
use dag::namedag::MemNameDag;
use dag::nameset::meta::MetaSet;
use dag::ops::DagAddHeads;
use dag::DagAlgorithm;
use dag::Set;
use dag::VertexName;
use indexedlog::{
    log::{self as ilog, IndexDef, IndexOutput, Log},
    DefaultOpenOptions,
};
use std::collections::HashMap;
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use types::mutation::MutationEntry;
use types::node::Node;
use vlqencoding::VLQDecodeAt;

pub use indexedlog::Repair;

pub struct MutationStore {
    log: Log,
    pending: Vec<MutationEntry>,
}

bitflags! {
    pub struct DagFlags: u8 {
        /// Include successors.
        const SUCCESSORS = 0b1;

        /// Include predecessors.
        const PREDECESSORS = 0b10;
    }
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
        let pending = Vec::new();
        Ok(MutationStore { log, pending })
    }

    /// Add an entry. Consider adding automatic entries based on this entry.
    /// See `flush` for automatic entries.
    pub fn add(&mut self, entry: &MutationEntry) -> Result<()> {
        self.add_raw(entry)?;
        self.pending.push(entry.clone());
        Ok(())
    }

    /// Add an entry. Do not consider adding automatic entries.
    pub fn add_raw(&mut self, entry: &MutationEntry) -> Result<()> {
        let mut buf = Vec::with_capacity(types::mutation::DEFAULT_ENTRY_SIZE);
        entry.serialize(&mut buf)?;
        self.log.append(buf.as_slice())?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        // If P -> Q, X -> Y are being added, and there is an existing chain P
        // -> ... -> X, add a Q -> Y marker automatically.
        // Note: P must not equal to X or Y.
        //
        // See also D7121487.

        // Prepare for calculation.
        let mut pred_map = HashMap::with_capacity(self.pending.len()); // node -> index
        let mut pred_nodes = Vec::with_capacity(self.pending.len());
        for (i, entry) in self.pending.iter().enumerate() {
            let pred = entry.preds[0];
            pred_map.insert(pred, i);
            pred_nodes.push(pred);
        }
        let pred_set =
            Set::from_static_names(pred_nodes.iter().map(|p| VertexName::copy_from(p.as_ref())));
        let dag = self.get_dag_advanced(pred_nodes, DagFlags::SUCCESSORS)?;
        let mut new_entries = Vec::new();

        // Scan through "X"s.
        for entry in &self.pending {
            let x = entry.preds[0];
            // Find all "P"s, as in P -> ... -> X, and X -> Y.
            let x_set = VertexName::copy_from(x.as_ref()).into();
            let x_ancestors = match dag.ancestors(x_set) {
                Ok(set) => set,
                Err(_) => continue, // might have cycles
            };
            let ps = x_ancestors & pred_set.clone();
            for p in ps.iter()? {
                let p = Node::from_slice(p?.as_ref())?;
                let y = entry.succ;
                if p == x || p == y {
                    continue;
                }
                let q = self.pending[pred_map[&p]].succ;
                if q == x || q == y || q == p {
                    continue;
                }
                // Copy P -> X to Q -> Y.
                let copy_entry = match self.get(x)? {
                    Some(entry) => entry,
                    _ => continue,
                };
                let op = if copy_entry.op.ends_with("-copy") {
                    copy_entry.op.clone()
                } else {
                    format!("{}-copy", &copy_entry.op)
                };
                // The new entry will be the one returned by `get(y)`.
                // It overrides the "x -> y" entry.
                let new_entry = MutationEntry {
                    succ: y,
                    preds: vec![x, q],
                    op,
                    ..copy_entry
                };
                new_entries.push(new_entry);
            }
        }

        let mut buf = Vec::with_capacity(types::mutation::DEFAULT_ENTRY_SIZE);
        for entry in new_entries {
            buf.clear();
            entry.serialize(&mut buf)?;
            self.log.append(buf.as_slice())?;
        }

        self.log.flush()?;
        self.pending.clear();
        Ok(())
    }

    fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            log: self.log.try_clone()?,
            pending: self.pending.clone(),
        })
    }

    /// Calculate the "obsolete" set, a subset of `draft` with visible successors.
    /// A vertex that is in `public` or `draft` is considered visible.
    ///
    /// For best performance, the callsite should consider calling
    /// `dag.sort(result)` to bind the `result` of this function to the main
    /// commit graph.
    pub fn calculate_obsolete(&self, public: Set, draft: Set) -> Result<Set> {
        let visible = public | draft.clone();
        let this = self.try_clone()?;

        // Evaluate `obsolete()` for all `draft`.
        // A draft is obsoleted if it has a visible successor.
        let evaluate = {
            let visible = visible.clone();
            move || -> dag::Result<Set> {
                // Vertex -> Node.
                let draft_nodes = draft
                    .iter()?
                    .filter_map(|v| v.ok().and_then(|v| Node::from_slice(v.as_ref()).ok()))
                    .collect::<Vec<Node>>();

                // Obtain the obsolete graph about draft successors.
                let obsdag = this
                    .get_dag_advanced(draft_nodes, DagFlags::SUCCESSORS)
                    .map_err(|e| dag::errors::BackendError::Other(e))?;

                // Filter out invisible successors.
                let obsvisible = obsdag.ancestors(obsdag.all()? & visible.clone())?;

                // Heads of `obsvisible` are not obsoleted. Other part (parent) of
                // `obsvisible` are obsoleted.
                let obsoleted = obsdag.parents(obsvisible)?;

                // Filter out unknown nodes.
                let obsoleted = draft.clone() & obsoleted;

                // Flatten the set for performance.
                Ok(obsoleted.flatten()?)
            }
        };

        // Spot check `obsolete()` for nodes.
        //
        // This is faster for revsets like `smallset & obsolete()`, where
        // smallset is way smaller than `draft()`.
        let contains_count = AtomicUsize::new(0);
        let this = self.try_clone()?;
        let contains = move |set: &MetaSet, v: &VertexName| -> dag::Result<bool> {
            // If "contains" is called a few times, calculate the full "obsolete()"
            // and use that instead.
            if contains_count.fetch_add(1, SeqCst) > 4 {
                set.evaluate()?.contains(v)
            } else if let Ok(id) = Node::from_slice(v.as_ref()) {
                let obsdag = this
                    .get_dag_advanced(vec![id], DagFlags::SUCCESSORS)
                    .map_err(|e| dag::errors::BackendError::Other(e))?;
                Ok((obsdag.all()? & visible.clone()).count()? > 1)
            } else {
                Ok(false)
            }
        };

        // The set has 2 code paths: contains, and full iteration.
        //
        // The contains test can be used to spot check a few nodes without
        // calculating the full set (ex. smallset & obsolete()).
        //
        // The full iteration is used in other cases.
        Ok(Set::from_evaluate_contains(evaluate, contains))
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
        self.get_dag_advanced(nodes, DagFlags::SUCCESSORS | DagFlags::PREDECESSORS)
    }

    /// Advanced version of `get_dag`. Specify whether to include successors or
    /// predecessors explicitly.
    pub fn get_dag_advanced(&self, nodes: Vec<Node>, flags: DagFlags) -> Result<MemNameDag> {
        // Raw parent map. Might contain cycles.
        let mut parent_map = HashMap::<Node, Vec<Node>>::new();
        let mut non_heads = HashSet::<Node>::new();
        let mut add_parent = |parent: &Node, child: &Node| {
            let parents = parent_map.entry(child.clone()).or_default();
            if !parents.contains(parent) {
                parents.push(parent.clone());
                non_heads.insert(parent.clone());
            }
        };

        // Visit nodes. Fill parent map.
        let mut to_visit = nodes;
        let mut connected = HashSet::new();
        while let Some(node) = to_visit.pop() {
            if !connected.insert(node.clone()) {
                continue;
            }
            if flags.contains(DagFlags::SUCCESSORS) {
                for entry in self.log.lookup(INDEX_PRED, &node)? {
                    let entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
                    add_parent(&node, &entry.succ);
                    to_visit.push(entry.succ);
                }
            }
            if flags.contains(DagFlags::PREDECESSORS) {
                for entry in self.log.lookup(INDEX_SUCC, &node)? {
                    let entry = MutationEntry::deserialize(&mut Cursor::new(entry?))?;
                    for pred in entry.preds {
                        add_parent(&pred, &node);
                        to_visit.push(pred);
                    }
                }
            }
        }

        // Construct parent_func.
        let parent_func = |node: VertexName| -> dag::Result<Vec<VertexName>> {
            match parent_map.get(&Node::from_slice(node.as_ref()).unwrap()) {
                None => Ok(Vec::new()),
                Some(parents) => Ok(parents
                    .iter()
                    .map(|n| VertexName::copy_from(n.as_ref()))
                    .collect()),
            }
        };
        let parent_func = dag::utils::break_parent_func_cycle(parent_func);

        // Calculate heads. This makes multiple things more efficient:
        // `add_heads`, `break_parent_func_cycle`, and the resulting graph.
        let mut heads: Vec<Node> = connected.difference(&non_heads).cloned().collect();
        heads.sort_unstable();
        let heads: Vec<VertexName> = heads
            .into_iter()
            .map(|n| VertexName::copy_from(n.as_ref()))
            .collect();

        // Construct the graph.
        let mut dag = MemNameDag::new();
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

    #[test]
    fn test_dag_cycle() -> Result<()> {
        let dir = TempDir::new("mutationstore")?;
        let mut ms = MutationStore::open(dir.path())?;

        for (pred, succ) in [("A", "B"), ("B", "C"), ("C", "A")].iter() {
            add(&mut ms, pred, succ)?;
        }
        ms.flush()?;

        // Nothing - cycles without a head is not rendered.
        assert_eq!(render(&ms, "A")?, "\n");
        assert_eq!(render(&ms, "B")?, "\n");
        assert_eq!(render(&ms, "C")?, "\n");

        // Add a "head" to "revive" the graph.
        add(&mut ms, "C", "D")?;
        ms.flush()?;
        assert_eq!(
            render(&ms, "D")?,
            r#"
            o  4444444444444444444444444444444444444444 (D)
            │
            o  4343434343434343434343434343434343434343 (C)
            │
            o  4242424242424242424242424242424242424242 (B)
            │
            o  4141414141414141414141414141414141414141 (A)"#
        );

        Ok(())
    }

    #[test]
    fn test_copy_entries() -> Result<()> {
        let dir = TempDir::new("mutationstore")?;
        let mut ms = MutationStore::open(dir.path())?;

        for (pred, succ) in [("P", "E"), ("E", "X")].iter() {
            add(&mut ms, pred, succ)?;
        }
        ms.flush()?;

        for (pred, succ) in [("P", "Q"), ("X", "Y")].iter() {
            add(&mut ms, pred, succ)?;
        }

        // Before flush, Q -> Y is not connected.
        assert_eq!(
            render(&ms, "P")?,
            r#"
            o  5959595959595959595959595959595959595959 (Y)
            │
            o  5858585858585858585858585858585858585858 (X)
            │
            o  4545454545454545454545454545454545454545 (E)
            │
            │ o  5151515151515151515151515151515151515151 (Q)
            ├─╯
            o  5050505050505050505050505050505050505050 (P)"#
        );

        // After flush, Q -> Y is auto created.
        ms.flush()?;
        assert_eq!(
            render(&ms, "P")?,
            r#"
            o    5959595959595959595959595959595959595959 (Y)
            ├─╮
            │ o  5151515151515151515151515151515151515151 (Q)
            │ │
            o │  5858585858585858585858585858585858585858 (X)
            │ │
            o │  4545454545454545454545454545454545454545 (E)
            ├─╯
            o  5050505050505050505050505050505050505050 (P)"#
        );

        Ok(())
    }

    #[test]
    fn test_calculate_obsolete() -> Result<()> {
        let dir = TempDir::new("mutationstore")?;
        let mut ms = MutationStore::open(dir.path())?;

        // C   F  # C -> F
        // |   |
        // B D E  # B -> D -> E
        //  \|/   # X -> Y
        // X Y Z  # Y, Z are public; X, B, E, C are draft

        for (pred, succ) in [("B", "D"), ("D", "E"), ("C", "F"), ("X", "Y")].iter() {
            add(&mut ms, pred, succ)?;
        }
        let public = Set::from_static_names(vec![v("Y"), v("Z")]);
        let draft = Set::from_static_names(vec![v("B"), v("E"), v("C"), v("X")]);
        let obsolete = ms.calculate_obsolete(public, draft)?;

        // B is obsoleted. It has a visible successor E (draft).
        assert!(obsolete.contains(&v("B"))?);

        // X is obsoleted. It has a visible successor Y (public).
        assert!(obsolete.contains(&v("X"))?);

        // C does not have a visible successor.
        assert!(!obsolete.contains(&v("C"))?);

        // D is not visible.
        assert!(!obsolete.contains(&v("D"))?);

        // A is not a draft.
        assert!(!obsolete.contains(&v("A"))?);

        // The set is not evaluated yet.
        assert_eq!(format!("{:?}", &obsolete), "<meta ?>");

        // E does not have a successor.
        assert!(!obsolete.contains(&v("E"))?);

        // Enough "contains" check. The set should be evaluated now.
        // (0x42 == b'B', 0x58 == b'X')
        assert_eq!(format!("{:.2?}", &obsolete), "<meta <static [42, 58]>>");

        Ok(())
    }

    /// Create a node from a single-char string.
    fn n(s: impl ToString) -> Node {
        Node::from_slice(s.to_string().repeat(Node::len()).as_bytes()).unwrap()
    }

    /// Create a vertex from a single-char string.
    fn v(s: impl ToString) -> VertexName {
        n(s).as_ref().to_vec().into()
    }

    /// Add (test) edges to the mutation store.
    fn add(ms: &mut MutationStore, pred: &str, succ: &str) -> Result<()> {
        ms.add(&MutationEntry {
            preds: vec![n(pred)],
            succ: n(succ),
            split: vec![],
            op: "rewrite".into(),
            user: "test".into(),
            time: 1,
            tz: -7200,
            extra: vec![],
        })
    }

    /// Render the mutation store for the given nodes.
    fn render(ms: &MutationStore, s: &str) -> Result<String> {
        let dag = ms.get_dag(s.chars().map(n).collect::<Vec<Node>>())?;
        renderdag::render_namedag(&dag, |v| Some(format!("({})", v.as_ref()[0] as char)))
    }
}
