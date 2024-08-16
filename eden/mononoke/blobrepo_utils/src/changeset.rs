/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_recursion::async_recursion;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use cloned::cloned;
use context::CoreContext;
use dashmap::DashMap;
use futures::try_join;
use futures::Stream;
use mercurial_types::HgChangesetId;
use slog::o;
use slog::Logger;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio_stream::wrappers::ReceiverStream;

use crate::bonsai::BonsaiMFVerifyVisitor;
use crate::BonsaiMFVerifyResult;
use crate::Repo;

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
pub fn visit_changesets<I, R: Repo>(
    ctx: CoreContext,
    logger: Logger,
    repo: R,
    visitor: BonsaiMFVerifyVisitor,
    start_points: I,
    follow_limit: usize,
) -> impl Stream<Item = Result<(BonsaiMFVerifyResult<R>, ChangesetVisitMeta)>> + Send
where
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
        visit_started: DashMap::new(),
    });

    for changeset_id in start_points {
        // Start off with follow_limit + 1 because that's logically the previous follow_remaining.
        let visit_one = VisitOne::new(ctx.clone(), &inner, changeset_id, follow_limit, &mut sender);
        if let Some(visit_one) = visit_one {
            tokio::spawn(visit_one.visit());
        }
    }

    ReceiverStream::new(receiver)
}

struct VisitOneShared<R> {
    logger: Logger,
    repo: R,
    visitor: BonsaiMFVerifyVisitor,
    visit_started: DashMap<HgChangesetId, ()>,
}

impl<R> VisitOneShared<R> {
    fn visit_started(&self, changeset_id: HgChangesetId) -> bool {
        self.visit_started.contains_key(&changeset_id)
    }

    fn mark_visit_started(&self, changeset_id: HgChangesetId) {
        self.visit_started.insert(changeset_id, ());
    }
}

struct VisitOne<R> {
    ctx: CoreContext,
    shared: Arc<VisitOneShared<R>>,
    logger: Logger,
    changeset_id: HgChangesetId,
    follow_remaining: usize,
    sender: Sender<Result<(BonsaiMFVerifyResult<R>, ChangesetVisitMeta)>>,
}

impl<R: Repo> VisitOne<R> {
    fn new(
        ctx: CoreContext,
        shared: &Arc<VisitOneShared<R>>,
        changeset_id: HgChangesetId,
        prev_follow_remaining: usize,
        sender: &mut Sender<Result<(BonsaiMFVerifyResult<R>, ChangesetVisitMeta)>>,
    ) -> Option<Self> {
        // Checks to figure out whether to terminate the visit.
        if prev_follow_remaining == 0 {
            return None;
        }
        if shared.visit_started(changeset_id) {
            return None;
        }
        if sender.is_closed() {
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

    #[async_recursion]
    async fn visit(self) -> Result<()> {
        let Self {
            ctx,
            shared,
            logger,
            changeset_id,
            follow_remaining,
            sender,
        } = self;

        shared.mark_visit_started(changeset_id);

        let parent_fut = {
            cloned!(ctx, shared, sender);
            async move {
                let parent_hashes = shared
                    .repo
                    .get_hg_changeset_parents(ctx.clone(), changeset_id)
                    .await?;

                for parent_id in parent_hashes {
                    cloned!(ctx, shared, mut sender);
                    let visit_one_fut = async move {
                        let visit_one = VisitOne::new(
                            ctx.clone(),
                            &shared,
                            parent_id,
                            follow_remaining,
                            &mut sender,
                        );
                        if let Some(visit_one) = visit_one {
                            visit_one.visit().await
                        } else {
                            Ok(())
                        }
                    };
                    // Avoid unbounded recursion by spawning separate futures for each parent
                    // directly on the executor.
                    tokio::spawn(visit_one_fut);
                }

                Ok(())
            }
        };

        let visit_fut = {
            cloned!(sender);
            async move {
                let changeset = changeset_id
                    .load(&ctx, shared.repo.repo_blobstore())
                    .await?;
                let repo = shared.repo.clone();
                let item = shared
                    .visitor
                    .clone()
                    .visit(ctx, logger, repo, changeset)
                    .await?;

                sender
                    .send(Ok((
                        item,
                        ChangesetVisitMeta {
                            changeset_id,
                            follow_remaining,
                        },
                    )))
                    .await
                    .map_err(anyhow::Error::msg)
            }
        };

        if let Err(err) = try_join!(visit_fut, parent_fut) {
            let _ = sender.send(Err(err)).await;
        }

        Ok(())
    }
}
