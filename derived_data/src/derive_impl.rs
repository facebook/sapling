// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::BonsaiDerived;
use crate::BonsaiDerivedMapping;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::{future, stream, Future, Stream};
use futures_ext::{bounded_traversal, FutureExt};
use mononoke_types::ChangesetId;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
use topo_sort::sort_topological;

/// Actual implementation of `BonsaiDerived::derive`, which recursively generates derivations.
/// If the data was already generated (i.e. the data is already in `derived_mapping`) then
/// nothing will be generated. Otherwise this function will generate data for this commit and for
/// all it's ancestors that didn't have this derived data.
///
/// TODO(T47650154) - add memcache leases to prevent deriving the data for the same commit at
///     the same time
/// TODO(T47650184) - log to scuba and ods how long it took to generate derived data
pub(crate) fn derive_impl<
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone,
>(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_mapping: Mapping,
    start_csid: ChangesetId,
) -> impl Future<Item = Derived, Error = Error> {
    // Find all ancestor commits that don't have derived data generated.
    // Note that they might not be topologically sorted.
    let changeset_fetcher = repo.get_changeset_fetcher();
    // This is necessary to avoid visiting the same commit a lot of times in mergy repos
    let visited: Arc<Mutex<HashSet<ChangesetId>>> = Arc::new(Mutex::new(HashSet::new()));

    bounded_traversal::bounded_traversal_stream(100, start_csid, {
        cloned!(ctx, derived_mapping);
        move |cs_id| {
            DeriveNode::from_bonsai(ctx.clone(), derived_mapping.clone(), cs_id).and_then({
                cloned!(ctx, changeset_fetcher, visited);
                move |derive_node| match derive_node {
                    DeriveNode::Derived(_) => future::ok((None, vec![])).left_future(),
                    DeriveNode::Bonsai(bcs_id) => changeset_fetcher
                        .get_parents(ctx.clone(), bcs_id)
                        .map({
                            cloned!(visited);
                            move |parents| {
                                let parents_to_visit: Vec<_> = {
                                    let mut visited = visited.lock().unwrap();
                                    parents
                                        .iter()
                                        .cloned()
                                        .filter(|p| visited.insert(*p))
                                        .collect()
                                };

                                // Topological sort needs parents, so return them here
                                (Some((bcs_id, parents)), parents_to_visit)
                            }
                        })
                        .right_future(),
                }
            })
        }
    })
    .filter_map(|x| x) // Remove all None
    .collect()
    .map(|v| {
        stream::iter_ok(
            sort_topological(&v.into_iter().collect::<HashMap<_, _>>())
                .expect("commit graph has cycles!")
                .into_iter()
                .rev(),
        )
    })
    .flatten_stream()
    .for_each({
        cloned!(ctx, derived_mapping, repo);
        move |bcs_id| derive_may_panic(ctx.clone(), repo.clone(), derived_mapping.clone(), bcs_id)
    })
    .and_then(move |()| fetch_derived_may_panic(ctx, start_csid, derived_mapping))
}

// Panics if any of the parents is not derived yet
fn derive_may_panic<Derived, Mapping>(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_mapping: Mapping,
    bcs_id: ChangesetId,
) -> impl Future<Item = (), Error = Error>
where
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone,
{
    let bcs_fut = repo.get_bonsai_changeset(ctx.clone(), bcs_id.clone());

    let changeset_fetcher = repo.get_changeset_fetcher();
    let derived_parents =
        changeset_fetcher
            .get_parents(ctx.clone(), bcs_id)
            .and_then({
                cloned!(ctx, derived_mapping);
                move |parents| {
                    future::join_all(parents.into_iter().map(move |p| {
                        fetch_derived_may_panic(ctx.clone(), p, derived_mapping.clone())
                    }))
                }
            });

    bcs_fut
        .join(derived_parents)
        .and_then({
            cloned!(ctx);
            move |(bcs, parents)| Derived::derive_from_parents(ctx, repo, bcs, parents)
        })
        .and_then(move |derived| derived_mapping.put(ctx, bcs_id, derived))
}

fn fetch_derived_may_panic<Derived, Mapping>(
    ctx: CoreContext,
    bcs_id: ChangesetId,
    derived_mapping: Mapping,
) -> impl Future<Item = Derived, Error = Error>
where
    Derived: BonsaiDerived,
    Mapping: BonsaiDerivedMapping<Value = Derived> + Send + Sync + Clone,
{
    DeriveNode::from_bonsai(ctx, derived_mapping, bcs_id).map(
        move |derive_node| match derive_node {
            DeriveNode::Derived(derived) => derived,
            DeriveNode::Bonsai(_) => {
                panic!("{} should be derived already", bcs_id);
            }
        },
    )
}

#[derive(Clone, Copy)]
enum DeriveNode<Derived> {
    /// Already derived value fetched from mapping
    Derived(Derived),
    /// Bonsai changeset which requires derivation
    Bonsai(ChangesetId),
}

impl<Derived: BonsaiDerived> DeriveNode<Derived> {
    fn from_bonsai<Mapping>(
        ctx: CoreContext,
        derived_mapping: Mapping,
        csid: ChangesetId,
    ) -> impl Future<Item = Self, Error = Error>
    where
        Mapping: BonsaiDerivedMapping<Value = Derived> + Clone,
    {
        // TODO: do not create intermediate hashmap, since this methods is going to be called
        //       most often, to get derived value
        derived_mapping
            .get(ctx, vec![csid.clone()])
            .map(move |csids_to_id| match csids_to_id.get(&csid) {
                Some(id) => DeriveNode::Derived(id.clone()),
                None => DeriveNode::Bonsai(csid),
            })
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bookmarks::BookmarkName;
    use fixtures::{
        branch_even, branch_uneven, branch_wide, linear, many_diamonds, many_files_dirs,
        merge_even, merge_uneven, unshared_merge_even, unshared_merge_uneven,
    };
    use futures_ext::BoxFuture;
    use maplit::hashmap;
    use mononoke_types::BonsaiChangeset;
    use revset::AncestorsNodeStream;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::runtime::Runtime;

    #[derive(Clone, Hash, Eq, Ord, PartialEq, PartialOrd)]
    struct TestGenNum(u64, ChangesetId, Vec<ChangesetId>);

    impl BonsaiDerived for TestGenNum {
        fn derive_from_parents(
            _ctx: CoreContext,
            _repo: BlobRepo,
            bonsai: BonsaiChangeset,
            parents: Vec<Self>,
        ) -> BoxFuture<Self, Error> {
            let parent_commits = parents.iter().map(|x| x.1).collect();

            future::ok(Self(
                parents.into_iter().max().map(|x| x.0).unwrap_or(0) + 1,
                bonsai.get_changeset_id(),
                parent_commits,
            ))
            .boxify()
        }
    }

    struct TestMapping {
        mapping: Arc<Mutex<HashMap<ChangesetId, TestGenNum>>>,
    }

    impl TestMapping {
        fn new() -> Self {
            Self {
                mapping: Arc::new(Mutex::new(hashmap! {})),
            }
        }
    }

    impl BonsaiDerivedMapping for TestMapping {
        type Value = TestGenNum;

        fn get(
            &self,
            _ctx: CoreContext,
            csids: Vec<ChangesetId>,
        ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
            let mut res = hashmap! {};
            {
                let mapping = self.mapping.lock().unwrap();
                for id in csids {
                    if let Some(gen_num) = mapping.get(&id) {
                        res.insert(id, gen_num.clone());
                    }
                }
            }

            future::ok(res).boxify()
        }

        fn put(
            &self,
            _ctx: CoreContext,
            csid: ChangesetId,
            id: Self::Value,
        ) -> BoxFuture<(), Error> {
            {
                let mut mapping = self.mapping.lock().unwrap();
                mapping.insert(csid, id);
            }
            future::ok(()).boxify()
        }
    }

    fn derive_for_master(runtime: &mut Runtime, ctx: CoreContext, repo: BlobRepo) {
        let master_book = BookmarkName::new("master").unwrap();
        let bcs_id = runtime
            .block_on(repo.get_bonsai_bookmark(ctx.clone(), &master_book))
            .unwrap()
            .unwrap();
        let expected = runtime
            .block_on(
                repo.get_changeset_fetcher()
                    .get_generation_number(ctx.clone(), bcs_id.clone()),
            )
            .unwrap();

        let mapping = Arc::new(TestMapping::new());
        let actual = runtime
            .block_on(TestGenNum::derive(
                ctx.clone(),
                repo.clone(),
                mapping.clone(),
                bcs_id,
            ))
            .unwrap();
        assert_eq!(expected.value(), actual.0);

        let changeset_fetcher = repo.get_changeset_fetcher();
        runtime
            .block_on(
                AncestorsNodeStream::new(
                    ctx.clone(),
                    &repo.get_changeset_fetcher(),
                    bcs_id.clone(),
                )
                .and_then(move |new_bcs_id| {
                    let parents = changeset_fetcher.get_parents(ctx.clone(), new_bcs_id.clone());
                    let mapping = mapping.get(ctx.clone(), vec![new_bcs_id]);

                    parents.join(mapping).map(move |(parents, mapping)| {
                        let gen_num = mapping.get(&new_bcs_id).unwrap();
                        assert_eq!(parents, gen_num.2);
                    })
                })
                .collect(),
            )
            .unwrap();
    }

    #[test]
    fn test_derive_linear() {
        let ctx = CoreContext::test_mock();
        let mut runtime = Runtime::new().unwrap();

        let repo = branch_even::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = branch_uneven::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = branch_wide::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = linear::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = many_files_dirs::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = merge_even::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = merge_uneven::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = unshared_merge_even::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = unshared_merge_uneven::getrepo();
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());

        let repo = many_diamonds::getrepo(&mut runtime);
        derive_for_master(&mut runtime, ctx.clone(), repo.clone());
    }
}
