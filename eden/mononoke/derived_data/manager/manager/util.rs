/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use context::SessionClass;
use mononoke_types::ChangesetId;

use super::DerivedDataManager;
use crate::derivable::BonsaiDerivable;
use crate::error::DerivationError;

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
        if self.config().types.contains(&Derivable::VARIANT) {
            Ok(())
        } else {
            Err(DerivationError::Disabled(
                Derivable::NAME,
                self.repo_id(),
                self.repo_name().to_string(),
            ))
        }
    }

    pub(super) fn check_blocked_derivation<Derivable>(
        &self,
        csids: &[ChangesetId],
    ) -> Result<(), DerivationError>
    where
        Derivable: BonsaiDerivable,
    {
        let derived_data_config = &self.repo_config().derived_data_config;
        for csid in csids {
            if derived_data_config.is_derivation_blocked(Derivable::VARIANT, *csid) {
                return Err(DerivationError::Blocked(Derivable::NAME, *csid));
            }
        }
        Ok(())
    }
}
