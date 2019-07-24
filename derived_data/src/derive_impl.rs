// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::BonsaiDerived;
use crate::BonsaiDerivedMapping;
use blobrepo::BlobRepo;
use changeset_fetcher::ChangesetFetcher;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use futures::{future, Future};
use futures_ext::{bounded_traversal, FutureExt};
use mononoke_types::ChangesetId;
use std::sync::Arc;

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
    DeriveNode::from_bonsai(ctx.clone(), derived_mapping.clone(), start_csid).and_then(
        move |init| {
            if let DeriveNode::Derived(id) = init {
                // derivation fetched from the cache
                return future::ok(id).left_future();
            }
            bounded_traversal::bounded_traversal(
                100,
                init,
                {
                    let changeset_fetcher = repo.get_changeset_fetcher();
                    cloned!(ctx, derived_mapping);
                    move |node| {
                        // FIXME - this code might have problems with very mergy repos.
                        // It may result in a combinatoral explosion in mergy repos, like the following:
                        //  o
                        //  |\
                        //  | o
                        //  |/|
                        //  o |
                        //  |\|
                        //  | o
                        //  |/|
                        //  o |
                        //  |\|
                        //  ...
                        //  |/|
                        //  | ~
                        //  o
                        //  |\
                        //  ~ ~
                        //
                        //
                        node.dependencies(
                            ctx.clone(),
                            derived_mapping.clone(),
                            changeset_fetcher.clone(),
                        )
                        .map(move |deps| (node, deps))
                    }
                },
                {
                    cloned!(ctx, repo);
                    move |node, parents| match node {
                        DeriveNode::Derived(id) => future::ok(id).left_future(),
                        DeriveNode::Bonsai(csid) => repo
                            .get_bonsai_changeset(ctx.clone(), csid)
                            .and_then({
                                cloned!(ctx, repo);
                                move |bonsai| {
                                    Derived::derive_from_parents(
                                        ctx,
                                        repo,
                                        bonsai,
                                        parents.collect(),
                                    )
                                }
                            })
                            .and_then({
                                cloned!(ctx, derived_mapping);
                                move |derived_id| {
                                    derived_mapping
                                        .put(ctx, csid, derived_id.clone())
                                        .map(move |_| derived_id)
                                }
                            })
                            .right_future(),
                    }
                },
            )
            .right_future()
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

    // dependencies which need to be computed before we can create derivation for current node
    fn dependencies<Mapping>(
        &self,
        ctx: CoreContext,
        derived_mapping: Mapping,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
    ) -> impl Future<Item = Vec<Self>, Error = Error>
    where
        Mapping: BonsaiDerivedMapping<Value = Derived> + Clone,
    {
        match self {
            DeriveNode::Derived(_) => future::ok(Vec::new()).left_future(),
            DeriveNode::Bonsai(csid) => changeset_fetcher
                .get_parents(ctx.clone(), *csid)
                .and_then(move |csids| {
                    derived_mapping
                        .get(ctx, csids.clone())
                        .map(move |mut csid_to_id| {
                            csids
                                .into_iter()
                                .map(|csid| match csid_to_id.remove(&csid) {
                                    Some(id) => DeriveNode::Derived(id.clone()),
                                    None => DeriveNode::Bonsai(csid),
                                })
                                .collect()
                        })
                })
                .right_future(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use bookmarks::BookmarkName;
    use fixtures::linear;
    use futures_ext::BoxFuture;
    use maplit::hashmap;
    use mononoke_types::BonsaiChangeset;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::runtime::Runtime;

    #[derive(Clone, Hash, Eq, Ord, PartialEq, PartialOrd)]
    struct TestGenNum(u64);

    impl BonsaiDerived for TestGenNum {
        fn derive_from_parents(
            _ctx: CoreContext,
            _repo: BlobRepo,
            _bonsai: BonsaiChangeset,
            parents: Vec<Self>,
        ) -> BoxFuture<Self, Error> {
            future::ok(Self(
                parents.into_iter().max().map(|x| x.0).unwrap_or(0) + 1,
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

    #[test]
    fn test_derive_linear() {
        let ctx = CoreContext::test_mock();
        let mut runtime = Runtime::new().unwrap();

        let repo = linear::getrepo();
        let master_book = BookmarkName::new("master").unwrap();
        let bcs_id = runtime
            .block_on(repo.get_bonsai_bookmark(ctx.clone(), &master_book))
            .unwrap()
            .unwrap();

        let res = runtime
            .block_on(TestGenNum::derive(
                ctx,
                repo,
                Arc::new(TestMapping::new()),
                bcs_id,
            ))
            .unwrap();
        assert_eq!(res.0, 11);
    }
}
