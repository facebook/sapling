/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # idmap
//!
//! See [`IdMap`] for the main structure.

use std::borrow::Cow;

use crate::errors::bug;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::ops::IdConvert;
use crate::ops::Parents;
use crate::segment::PreparedFlatSegments;
use crate::types_ext::PreparedFlatSegmentsExt;
use crate::Error;
use crate::IdSet;
use crate::Result;

#[cfg(any(test, feature = "indexedlog-backend"))]
mod indexedlog_idmap;
mod mem_idmap;

#[cfg(any(test, feature = "indexedlog-backend"))]
pub use indexedlog_idmap::IdMap;
pub(crate) use mem_idmap::CoreMemIdMap;
pub use mem_idmap::MemIdMap;

/// DAG-aware write operations.
#[async_trait::async_trait]
pub trait IdMapAssignHead: IdConvert + IdMapWrite {
    /// Assign an id for a head in a DAG. This implies ancestors of the
    /// head will also have ids assigned.
    ///
    /// This function is incremental. If the head or any of its ancestors
    /// already have an id stored in this map, the existing ids will be
    /// reused.
    ///
    /// This function needs roughly `O(N)` heap memory. `N` is the number of
    /// ids to assign. When `N` is very large, try assigning ids to a known
    /// ancestor first.
    ///
    /// New `id`s inserted by this function will have the specified `group`.
    /// Existing `id`s that are ancestors of `head` will get re-assigned
    /// if they have a higher `group`.
    ///
    /// `covered_ids` specifies what ranges of `Id`s are already covered.
    /// This is usually obtained from `IdDag::all_ids_in_groups(&Group::ALL)`.
    /// `IdMap` itself might not be able to provide that information
    /// efficiently because it might be lazy. `covered_ids` will be updated
    /// to cover newly inserted `Id`s.
    ///
    /// `reserved_ids` specifies what ranges are reserved for future growth
    /// of other important heads (usually a couple of mainline branches that
    /// are long-lived, growing, and used by many people). This is useful
    /// to reduce fragmentation.
    async fn assign_head(
        &mut self,
        head: VertexName,
        parents_by_name: &dyn Parents,
        group: Group,
        covered_ids: &mut IdSet,
        reserved_ids: &IdSet,
    ) -> Result<PreparedFlatSegments> {
        // There are some interesting cases to optimize the numbers:
        //
        // C     For a merge C, it has choice to assign numbers to A or B
        // |\    first (A and B are abstract branches that have many nodes).
        // A B   Suppose branch A is linear and B have merges, and D is
        // |/    (::A & ::B). Then:
        // D
        //
        // - If `D` is empty or already assigned, it's better to assign A last.
        //   This is because (A+C) can then always form a segment regardless of
        //   the complexity of B:
        //
        //      B   A   C       vs.        A   B   C
        //     ~~~  ^^^^^                     ~~~
        //     xxxxxx                          *****
        //                                 xxxxx
        //
        //   [~]: Might be complex (ex. many segments)
        //   [^]: Can always form a segment. (better)
        //   [*]: Can only be a segment if segment size is large enough.
        //   [x]: Cannot form a segment.
        //
        // - If `D` is not empty (and not assigned), it _might_ be better to
        //   assign D and A first. This provides benefits for A and D to be
        //   continuous, with the downside that A and C are not continuous.
        //
        //   A typical pattern is one branch continuously merges into the other
        //   (see also segmented-changelog.pdf, page 19):
        //
        //        B---D---F
        //         \   \   \
        //      A---C---E---G
        //
        // The code below is optimized for cases where p1 branch is linear,
        // but p2 branch is not.
        //
        // However, the above visit order (first parent last) is not optimal
        // for incremental build case with pushrebase branches. Because
        // pushrebase uses the first parent as the mainline. For example:
        //
        //    A---B---C-...-D---M  (parents(M) = [D, F])
        //                     /
        //              E-...-F
        //
        // The A ... M branch is the mainline. The head of the mainline
        // was A ... D, then M. An incremental build job might have built up
        // A, B, ..., D before it sees M. In this case it's better to make
        // the incremental build finish the A ... D part before jumping to
        // E ... F.
        //
        // We choose first parent last order if `covered` is empty, or when
        // visiting ancestors of non-first parents.
        let mut outcome = PreparedFlatSegments::default();

        #[derive(Copy, Clone, Debug)]
        enum VisitOrder {
            /// Visit the first parent first.
            FirstFirst,
            /// Visit the first parent last.
            FirstLast,
        }

        // Emulate the stack in heap to avoid overflow.
        #[derive(Debug)]
        enum Todo {
            /// Visit parents. Finally assign self. This will eventually turn into AssignedId.
            Visit {
                head: VertexName,

                /// The `Id` in `IdMap` that is known assigned to the `head`.
                /// This can be non-`None` if `IdMap` has more entries than `IdDag`.
                known_id: Option<Id>,

                order: VisitOrder,
            },

            /// Assign an `Id` if not assigned. Their parents are prepared in the
            /// `parent_ids` stack. `Assign` `head` and `Visit` `head`'s parents
            /// are pushed together so the `Visit` entries can turn into `Id`s in
            /// the `parent_ids` stack.
            Assign {
                /// The vertex to assign. Its parents are already visited and assigned.
                head: VertexName,

                /// The `Id` in `IdMap` that is known assigned to the `head`.
                /// This can be non-`None` if `IdMap` has more entries than `IdDag`.
                known_id: Option<Id>,

                /// The number of parents, at the end of the `parent_ids`.
                parent_len: usize,

                /// The order of parents if extracted from `parent_ids`.
                order: VisitOrder,
            },

            /// Assigned Id. Will be picked by and pushed to the current `parent_ids` stack.
            AssignedId { id: Id },
        }
        use Todo::Assign;
        use Todo::AssignedId;
        use Todo::Visit;
        let mut parent_ids: Vec<Id> = Vec::new();

        let mut todo_stack: Vec<Todo> = {
            let order = if covered_ids.is_empty() {
                // Assume re-building from scratch.
                VisitOrder::FirstLast
            } else {
                // Assume incremental updates with pushrebase.
                VisitOrder::FirstFirst
            };
            vec![Visit {
                head: head.clone(),
                known_id: None,
                order,
            }]
        };
        while let Some(todo) = todo_stack.pop() {
            tracing::trace!(target: "dag::assign", "todo: {:?}", &todo);
            match todo {
                Visit {
                    head,
                    known_id,
                    order,
                } => {
                    // If the id was not assigned, or was assigned to a higher group,
                    // (re-)assign it to this group.
                    //
                    // PERF: This might trigger remote fetch too frequently.
                    let known_id = match known_id {
                        Some(id) => Some(id),
                        None => self.vertex_id_with_max_group(&head, group).await?,
                    };
                    match known_id {
                        Some(id) if covered_ids.contains(id) => todo_stack.push(AssignedId { id }),
                        _ => {
                            let parents = parents_by_name.parent_names(head.clone()).await?;
                            tracing::trace!(target: "dag::assign", "visit {:?} ({:?}) with parents {:?}", &head, known_id, &parents);
                            todo_stack.push(Assign {
                                head,
                                known_id,
                                parent_len: parents.len(),
                                order,
                            });
                            let mut visit = parents;
                            match order {
                                VisitOrder::FirstFirst => {}
                                VisitOrder::FirstLast => visit.reverse(),
                            }
                            for (i, p) in visit.into_iter().enumerate() {
                                // If the parent was not assigned, or was assigned to a higher group,
                                // (re-)assign the parent to this group.
                                match self.vertex_id_with_max_group(&p, group).await {
                                    Ok(Some(id)) if covered_ids.contains(id) => {
                                        todo_stack.push(AssignedId { id })
                                    }
                                    // Go deeper if IdMap has the entry but IdDag misses it.
                                    Ok(Some(id)) => todo_stack.push(Visit {
                                        head: p,
                                        known_id: Some(id),
                                        order,
                                    }),
                                    Ok(None) => {
                                        let parent_order = match (order, i) {
                                            (VisitOrder::FirstFirst, 0) => VisitOrder::FirstFirst,
                                            _ => VisitOrder::FirstLast,
                                        };
                                        todo_stack.push(Visit {
                                            head: p,
                                            known_id: None,
                                            order: parent_order,
                                        })
                                    }
                                    Err(e) => return Err(e),
                                }
                            }
                        }
                    }
                }
                Assign {
                    head,
                    known_id,
                    parent_len,
                    order,
                } => {
                    let parent_start = parent_ids.len() - parent_len;
                    let known_id = match known_id {
                        Some(id) => Some(id),
                        None => self.vertex_id_with_max_group(&head, group).await?,
                    };
                    let id = match known_id {
                        Some(id) if covered_ids.contains(id) => id,
                        _ => {
                            let parents = match order {
                                VisitOrder::FirstLast => Cow::Borrowed(&parent_ids[parent_start..]),
                                VisitOrder::FirstFirst => Cow::Owned(
                                    parent_ids[parent_start..]
                                        .iter()
                                        .cloned()
                                        .rev()
                                        .collect::<Vec<_>>(),
                                ),
                            };
                            let id = match known_id {
                                Some(id) => id,
                                None => {
                                    let candidate_id = match parents.iter().max() {
                                        Some(&max_parent_id) => {
                                            (max_parent_id + 1).max(group.min_id())
                                        }
                                        None => group.min_id(),
                                    };
                                    adjust_candidate_id(
                                        self,
                                        covered_ids,
                                        reserved_ids,
                                        candidate_id,
                                    )
                                    .await?
                                }
                            };
                            if id.group() != group {
                                return Err(Error::IdOverflow(group));
                            }
                            covered_ids.push(id);
                            if known_id.is_none() {
                                tracing::trace!(target: "dag::assign", "assign {:?} = {:?}", &head, id);
                                self.insert(id, head.as_ref()).await?;
                            } else {
                                tracing::trace!(target: "dag::assign", "assign {:?} = {:?} (known)", &head, id);
                            }
                            if parents.iter().any(|&p| p >= id) {
                                return bug(format!(
                                    "IdMap Ids are not topo-sorted: {:?} ({:?}) has parent ids {:?}",
                                    id, head, &parents,
                                ));
                            }
                            outcome.push_edge(id, &parents);
                            id
                        }
                    };
                    parent_ids.truncate(parent_start);
                    todo_stack.push(AssignedId { id });
                }
                AssignedId { id } => {
                    if !covered_ids.contains(id) {
                        return bug(format!(
                            concat!(
                                "attempted to assign id with wrong order: {:?} ",
                                "is being pushed as parent id but it cannot be used ",
                                "because it is not yet covered by IdDag",
                            ),
                            &id
                        ));
                    }
                    parent_ids.push(id);
                }
            }
        }

        Ok(outcome)
    }
}

/// Pick a minimal `n`, so `candidate_id + n` is an `Id` that is not "covered",
/// not "reserved", and not in the "map".  Return the picked `Id`.
async fn adjust_candidate_id(
    map: &(impl IdConvert + ?Sized),
    covered_ids: &IdSet,
    reserved_ids: &IdSet,
    mut candidate_id: Id,
) -> Result<Id> {
    loop {
        // (Fast) test using covered_ids + reserved_ids.
        loop {
            if let Some(span) = covered_ids.span_contains(candidate_id) {
                candidate_id = span.high + 1;
                continue;
            }
            if let Some(span) = reserved_ids.span_contains(candidate_id) {
                candidate_id = span.high + 1;
                continue;
            }
            break;
        }
        // (Slow) test using the IdMap.
        let new_candidate_id = ensure_id_not_exist_in_map(map, candidate_id).await?;
        if new_candidate_id == candidate_id {
            break;
        } else {
            // Check the covered_ids + reserved_ids.
            candidate_id = new_candidate_id;
        }
    }
    Ok(candidate_id)
}

/// Pick a minimal `n`, so `candidate_id + n` is an `Id` that is not in the
/// "map". Return the picked `Id`.
async fn ensure_id_not_exist_in_map(
    map: &(impl IdConvert + ?Sized),
    mut candidate_id: Id,
) -> Result<Id> {
    // PERF: This lacks of batching if it forms a loop. But it
    // is also expected to be rare - only when the server
    // tailer (assuming only one tailer is writing globally) is
    // killed abnormally, *and* the branch being assigned has
    // non-fast-forward move, this code path becomes useful.
    //
    // Technically, not using `locally` is more correct in a
    // lazy `IdMap`. However, lazy `IdMap` is only used by
    // client (local) dag, which ensures `IdMap` and `IdDag`
    // are in-sync, meaning that the above `covered_ids` check
    // is sufficient. So this is really only protecting the
    // server's out-of-sync `IdMap` use-case, where the
    // `locally` variant is the same as the non-`locally`,
    // since the server has a non-lazy `IdMap`.
    while let [true] = &map.contains_vertex_id_locally(&[candidate_id]).await?[..] {
        candidate_id = candidate_id + 1;
    }
    Ok(candidate_id)
}

impl<T> IdMapAssignHead for T where T: IdConvert + IdMapWrite {}

/// Write operations for IdMap.
#[async_trait::async_trait]
pub trait IdMapWrite {
    /// Insert a new `(id, name)` pair to the map.
    ///
    /// The `id` and `name` mapping should be unique, it's an error to map an id
    /// to multiple names, or map a name to multiple ids. Note: older versions
    /// of `IdMap` allowed mapping a name to a non-master Id, then a master Id
    /// (in this order), and the master Id is used for lookups. This is no
    /// longer permitted.
    async fn insert(&mut self, id: Id, name: &[u8]) -> Result<()>;
    /// Remove ids in the range `low..=high` and their associated names.
    /// Return removed names.
    async fn remove_range(&mut self, low: Id, high: Id) -> Result<Vec<VertexName>>;
}

#[cfg(test)]
mod tests {
    use nonblocking::non_blocking_result as r;
    use tempfile::tempdir;

    use super::*;
    use crate::ops::Persist;
    use crate::ops::PrefixLookup;

    #[cfg(all(test, feature = "indexedlog-backend"))]
    #[test]
    fn test_basic_operations() {
        let dir = tempdir().unwrap();
        let mut map = IdMap::open(dir.path()).unwrap();
        let lock = map.lock().unwrap();
        map.reload(&lock).unwrap();
        map.insert(Id(1), b"abc").unwrap();
        map.insert(Id(2), b"def").unwrap();
        map.insert(Id(10), b"ghi").unwrap();
        map.insert(Id(11), b"ghi").unwrap_err(); // ghi maps to 10
        map.insert(Id(10), b"ghi2").unwrap_err(); // 10 maps to ghi

        // Test another group.
        let id = Group::NON_MASTER.min_id();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl2").unwrap_err(); // id maps to jkl
        map.insert(id + 1, b"jkl2").unwrap();
        map.insert(id + 2, b"jkl2").unwrap_err(); // jkl2 maps to id + 1
        map.insert(Id(15), b"jkl2").unwrap_err(); // reassign jkl2 to master group - error.
        map.insert(id + 3, b"abc").unwrap_err(); // reassign abc to non-master group - error.

        // Test hex lookup.
        assert_eq!(0x6a, b'j');
        assert_eq!(
            r(map.vertexes_by_hex_prefix(b"6a", 3)).unwrap(),
            [
                VertexName::from(&b"jkl"[..]),
                VertexName::from(&b"jkl2"[..])
            ]
        );
        assert_eq!(
            r(map.vertexes_by_hex_prefix(b"6a", 1)).unwrap(),
            [VertexName::from(&b"jkl"[..])]
        );
        assert!(r(map.vertexes_by_hex_prefix(b"6b", 1)).unwrap().is_empty());

        for _ in 0..=1 {
            assert_eq!(map.find_name_by_id(Id(1)).unwrap().unwrap(), b"abc");
            assert_eq!(map.find_name_by_id(Id(2)).unwrap().unwrap(), b"def");
            assert!(map.find_name_by_id(Id(3)).unwrap().is_none());
            assert_eq!(map.find_name_by_id(Id(10)).unwrap().unwrap(), b"ghi");

            assert_eq!(map.find_id_by_name(b"abc").unwrap().unwrap().0, 1);
            assert_eq!(map.find_id_by_name(b"def").unwrap().unwrap().0, 2);
            assert_eq!(map.find_id_by_name(b"ghi").unwrap().unwrap().0, 10);
            assert_eq!(map.find_id_by_name(b"jkl").unwrap().unwrap(), id);
            assert_eq!(
                format!("{:?}", map.find_id_by_name(b"jkl2").unwrap().unwrap()),
                "N1"
            );
            assert!(map.find_id_by_name(b"jkl3").unwrap().is_none());
        }

        // Test Debug
        assert_eq!(
            format!("{:?}", &map),
            r#"IdMap {
  abc: 1,
  def: 2,
  ghi: 10,
  jkl: N0,
  jkl2: N1,
}
"#
        );
    }

    #[test]
    fn test_remove_range() {
        let map = MemIdMap::new();
        check_remove_range(map);

        #[cfg(feature = "indexedlog-backend")]
        {
            let dir = tempdir().unwrap();
            let path = dir.path();
            let map = IdMap::open(path).unwrap();
            check_remove_range(map);
        }
    }

    fn check_remove_range(mut map: impl IdConvert + IdMapWrite) {
        let items: &[(Id, &[u8])] = &[
            (Id(0), b"z"),
            (Id(1), b"a"),
            (Id(2), b"bbb"),
            (Id(3), b"bb"),
            (Id(4), b"cc"),
            (Id(5), b"ccc"),
            (Id(9), b"ddd"),
            (Id(11), b"e"),
            (Id(13), b"ff"),
            (nid(0), b"n"),
            (nid(1), b"n1"),
            (nid(2), b"n2"),
            (nid(3), b"n3"),
            (nid(4), b"n4"),
            (nid(5), b"n5"),
            (nid(12), b"n12"),
            (nid(20), b"n20"),
        ];
        for (id, name) in items {
            r(map.insert(*id, name)).unwrap();
        }

        // deleted ids in a string, with extra consistency checks.
        let deleted = |map: &dyn IdConvert| -> String {
            let mut deleted_ids = Vec::new();
            for (id, name) in items {
                let name = VertexName::copy_from(name);
                let id = *id;
                let has_id = r(map.contains_vertex_id_locally(&[id])).unwrap()[0];
                let lookup_id = r(map.vertex_id_optional(&name)).unwrap();
                let lookup_name = if has_id {
                    Some(r(map.vertex_name(id)).unwrap())
                } else {
                    None
                };

                match (lookup_id, lookup_name) {
                    (None, None) => deleted_ids.push(id),
                    (None, Some(_)) => {
                        panic!("name->id deleted but not id->name: ({:?} {:?})", id, name)
                    }
                    (Some(_), None) => {
                        panic!("id->name deleted but not name->id: ({:?} {:?})", id, name)
                    }
                    (Some(lid), Some(lname)) => {
                        assert_eq!(lid, id);
                        assert_eq!(lname, name);
                    }
                }
            }
            format!("{:?}", deleted_ids)
        };

        let f = |vs: Vec<VertexName>| -> String {
            let mut vs = vs;
            vs.sort_unstable();
            format!("{:?}", vs)
        };

        let removed = r(map.remove_range(Id(1), Id(3))).unwrap();
        assert_eq!(f(removed), "[a, bb, bbb]");
        assert_eq!(deleted(&map), "[1, 2, 3]");

        let removed = r(map.remove_range(Id(8), Id(12))).unwrap();
        assert_eq!(f(removed), "[ddd, e]");
        assert_eq!(deleted(&map), "[1, 2, 3, 9, 11]");

        let removed = r(map.remove_range(nid(2), nid(4))).unwrap();
        assert_eq!(f(removed), "[n2, n3, n4]");
        assert_eq!(deleted(&map), "[1, 2, 3, 9, 11, N2, N3, N4]");

        let removed = r(map.remove_range(nid(20), nid(10000))).unwrap();
        assert_eq!(f(removed), "[n20]");
        assert_eq!(deleted(&map), "[1, 2, 3, 9, 11, N2, N3, N4, N20]");
    }

    fn nid(i: u64) -> Id {
        Group::NON_MASTER.min_id() + i
    }
}
