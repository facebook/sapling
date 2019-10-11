/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use chashmap::CHashMap;
use cloned::cloned;
use context::CoreContext;
use failure::Error;
use failure_ext::Result;
use futures::{
    sync::mpsc::{self, Sender},
    Future, Stream,
};
use futures_ext::{send_discard, BoxFuture};
use mercurial_types::{blobs::HgBlobChangeset, HgChangesetId};
use slog::{o, Logger};
use std::sync::Arc;

/// This trait enables parallelized walks over changesets.
pub trait ChangesetVisitor: Clone + Send + Sync + 'static {
    type Item: Send;

    /// Visit a specific changeset.
    ///
    /// `logger` is already customized to be specific to this changeset.
    ///
    /// Each visit instance will get a fresh copy of the changeset visitor -- this is unfortunately
    /// unavoidable due to the way tokio works. To share state between instances, use an Arc.
    fn visit(
        self,
        ctx: CoreContext,
        logger: Logger,
        repo: BlobRepo,
        changeset: HgBlobChangeset,
        follow_remaining: usize,
    ) -> BoxFuture<Self::Item, Error>;
}

/// Information about the specific changeset whose result is provided.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangesetVisitMeta {
    pub changeset_id: HgChangesetId,
    pub follow_remaining: usize,
}

/// Walk over changesets in parallel, calling the visitor for each changeset.
///
/// Behind this scenes, this uses the default tokio executor (which is typically a thread pool, so
/// this is typically highly parallel). Dropping the returned stream will cause further visiting to
/// be canceled.
pub fn visit_changesets<V, I>(
    ctx: CoreContext,
    logger: Logger,
    repo: BlobRepo,
    visitor: V,
    start_points: I,
    follow_limit: usize,
) -> impl Stream<Item = (V::Item, ChangesetVisitMeta), Error = Error> + Send
where
    V: ChangesetVisitor,
    I: IntoIterator<Item = HgChangesetId>,
{
    // Some notes about this:
    //
    // * mpsc::channels will automatically end the stream when all senders are dropped. Each
    // individual visit call will have a sender associated with it, and when all verifications
    // are complete there will be no more senders, so it all works out.
    //
    // * The buffer is set to a relatively small number because the actual size of the buffer
    // is the number passed in here plus the number of senders that exist, and VisitOne below
    // creates at least one sender per changeset it traverses. If this proves to be a problem,
    // it might be worth wrapping the sender up in an Arc or similar.
    let (mut sender, receiver) = mpsc::channel(16);

    let inner = Arc::new(VisitOneShared {
        logger,
        repo,
        visitor,
        visit_started: CHashMap::new(),
    });

    for changeset_id in start_points {
        // Start off with follow_limit + 1 because that's logically the previous follow_remaining.
        let visit_one = VisitOne::new(ctx.clone(), &inner, changeset_id, follow_limit, &mut sender);
        if let Some(visit_one) = visit_one {
            tokio::spawn(visit_one.visit());
        }
    }

    receiver
        .map_err(|()| unreachable!("Receiver can never produce errors"))
        .and_then(|res| res)
}

struct VisitOneShared<V> {
    logger: Logger,
    repo: BlobRepo,
    visitor: V,
    visit_started: CHashMap<HgChangesetId, ()>,
}

impl<V> VisitOneShared<V> {
    #[inline]
    fn visit_started(&self, changeset_id: HgChangesetId) -> bool {
        self.visit_started.contains_key(&changeset_id)
    }

    #[inline]
    fn mark_visit_started(&self, changeset_id: HgChangesetId) {
        self.visit_started.insert(changeset_id, ());
    }
}

struct VisitOne<V>
where
    V: ChangesetVisitor,
{
    ctx: CoreContext,
    shared: Arc<VisitOneShared<V>>,
    logger: Logger,
    changeset_id: HgChangesetId,
    follow_remaining: usize,
    sender: Sender<Result<(V::Item, ChangesetVisitMeta)>>,
}

impl<V> VisitOne<V>
where
    V: ChangesetVisitor,
{
    fn new(
        ctx: CoreContext,
        shared: &Arc<VisitOneShared<V>>,
        changeset_id: HgChangesetId,
        prev_follow_remaining: usize,
        sender: &mut Sender<Result<(V::Item, ChangesetVisitMeta)>>,
    ) -> Option<Self> {
        // Checks to figure out whether to terminate the visit.
        if prev_follow_remaining == 0 {
            return None;
        }
        if shared.visit_started(changeset_id) {
            return None;
        }
        if let Err(_) = sender.poll_ready() {
            // The receiver is closed, so there's no point doing anything.
            return None;
        }

        let logger = shared
            .logger
            .new(o!["changeset_id" => format!("{}", changeset_id)]);

        Some(Self {
            ctx,
            shared: shared.clone(),
            logger,
            changeset_id,
            follow_remaining: prev_follow_remaining - 1,
            sender: sender.clone(),
        })
    }

    fn visit(self) -> impl Future<Item = (), Error = ()> + Send {
        let Self {
            ctx,
            shared,
            logger,
            changeset_id,
            follow_remaining,
            sender,
        } = self;

        shared.mark_visit_started(changeset_id);

        let parents_fut = shared
            .repo
            .get_changeset_parents(ctx.clone(), changeset_id)
            .map({
                cloned!(ctx, shared, mut sender);
                move |parent_hashes| {
                    for parent_id in parent_hashes {
                        let visit_one = VisitOne::new(
                            ctx.clone(),
                            &shared,
                            parent_id,
                            follow_remaining,
                            &mut sender,
                        );
                        if let Some(visit_one) = visit_one {
                            // Avoid unbounded recursion by spawning separate futures for each parent
                            // directly on the executor.
                            tokio::spawn(visit_one.visit());
                        }
                    }
                }
            });

        let visit_fut = shared
            .repo
            .get_changeset_by_changesetid(ctx.clone(), changeset_id)
            .and_then({
                cloned!(ctx, shared.visitor, shared.repo);
                move |changeset| visitor.visit(ctx, logger, repo, changeset, follow_remaining)
            })
            .and_then({
                let sender = sender.clone();
                move |item| {
                    send_discard(
                        sender,
                        Ok((
                            item,
                            ChangesetVisitMeta {
                                changeset_id,
                                follow_remaining,
                            },
                        )),
                    )
                }
            });

        visit_fut
            .join(parents_fut)
            .map(|((), ())| ())
            .or_else(move |err| send_discard(sender, Err(err)))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}

        assert_send::<VisitOneShared<()>>();
        assert_sync::<VisitOneShared<()>>();
    }
}
