/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::Result;
use context::{CoreContext, SessionClass};
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::derivable::BonsaiDerivable;
use crate::error::DerivationError;

use super::DerivedDataManager;

impl DerivedDataManager {
    /// Returns the passed-in `CoreContext` with the session class modified to
    /// the one that should be used for derivation.
    pub(super) fn set_derivation_session_class(&self, mut ctx: CoreContext) -> CoreContext {
        if tunables::tunables()
            .get_by_repo_derived_data_use_background_session_class(self.repo_name())
            .unwrap_or(false)
        {
            ctx.session_mut()
                .override_session_class(SessionClass::BackgroundUnlessTooSlow);
        }
        ctx
    }

    /// Construct a scuba sample builder for logging derivation of this
    /// changeset to the derived data scuba table.
    pub(super) fn derived_data_scuba<Derivable>(
        &self,
        csid: ChangesetId,
    ) -> MononokeScubaSampleBuilder
    where
        Derivable: BonsaiDerivable,
    {
        let mut scuba = self.inner.scuba.clone();
        scuba.add("derived_data", Derivable::NAME);
        scuba.add("changeset", csid.to_string());
        scuba
    }

    pub(super) fn check_enabled<Derivable>(&self) -> Result<(), DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        if self.config().types.contains(Derivable::NAME) {
            Ok(())
        } else {
            Err(DerivationError::Disabled(
                Derivable::NAME,
                self.repo_id(),
                self.repo_name().to_string(),
            ))
        }
    }

    pub(super) fn max_parallel_derivations(&self) -> usize {
        let buffer_size = tunables::tunables().get_derived_data_parallel_derivation_buffer();
        if buffer_size > 0 {
            buffer_size
                .try_into()
                .expect("buffer size should convert to usize")
        } else {
            10
        }
    }
}
