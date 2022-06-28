/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use changesets::ChangesetsArc;
use context::CoreContext;
use ephemeral_blobstore::Bubble;
use ephemeral_blobstore::EphemeralChangesets;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use std::sync::Arc;

use super::DerivationAssigner;
use super::DerivationAssignment;
use super::DerivedDataManager;
use super::DerivedDataManagerInner;
use super::SecondaryManagerData;

struct BubbleAssigner {
    changesets: Arc<EphemeralChangesets>,
}

#[async_trait::async_trait]
impl DerivationAssigner for BubbleAssigner {
    async fn assign(
        &self,
        _ctx: &CoreContext,
        cs: Vec<ChangesetId>,
    ) -> anyhow::Result<DerivationAssignment> {
        let in_bubble = self.changesets.fetch_gens(&cs).await?;
        let (in_bubble, not_in_bubble) = cs
            .into_iter()
            .partition(|cs_id| in_bubble.contains_key(cs_id));
        Ok(DerivationAssignment {
            primary: not_in_bubble,
            secondary: in_bubble,
        })
    }
}

impl DerivedDataManager {
    pub fn for_bubble(
        self,
        bubble: Bubble,
        // Perhaps this can be fetched from inside the manager in the future
        container: impl ChangesetsArc + RepoIdentityRef + RepoBlobstoreRef,
    ) -> Self {
        let changesets = Arc::new(bubble.changesets(container));
        Self {
            inner: Arc::new(DerivedDataManagerInner {
                secondary: Some(SecondaryManagerData {
                    manager: Self {
                        inner: Arc::new(DerivedDataManagerInner {
                            changesets: changesets.clone(),
                            repo_blobstore: bubble
                                .wrap_repo_blobstore(self.inner.repo_blobstore.clone()),
                            filenodes: None,
                            bonsai_hg_mapping: None,
                            ..self.inner.as_ref().clone()
                        }),
                    },
                    assigner: Arc::new(BubbleAssigner { changesets }),
                }),
                ..self.inner.as_ref().clone()
            }),
        }
    }
}
