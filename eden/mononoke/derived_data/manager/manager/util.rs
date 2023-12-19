/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Result;
use context::CoreContext;
use context::SessionClass;
use scuba_ext::MononokeScubaSampleBuilder;

use super::DerivedDataManager;
use crate::derivable::BonsaiDerivable;
use crate::error::DerivationError;

#[derive(Clone, Debug)]
pub struct DiscoveryStats {
    pub(crate) find_underived_completion_time: Duration,
    pub(crate) commits_discovered: u32,
}

impl DiscoveryStats {
    pub(crate) fn add_scuba_fields(&self, builder: &mut MononokeScubaSampleBuilder) {
        builder.add(
            "find_underived_completion_time_ms",
            self.find_underived_completion_time.as_millis() as u64,
        );
        builder.add("commits_discovered", self.commits_discovered);
    }
}

impl DerivedDataManager {
    /// Returns the passed-in `CoreContext` with the session class modified to
    /// the one that should be used for derivation.
    pub(super) fn set_derivation_session_class(&self, mut ctx: CoreContext) -> CoreContext {
        if justknobs::eval(
            "scm/mononoke:derived_data_use_background_session_class",
            None,
            Some(self.repo_name()),
        )
        .unwrap_or_default()
        {
            ctx.session_mut()
                .override_session_class(SessionClass::BackgroundUnlessTooSlow);
        }
        ctx
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
        10
    }
}
