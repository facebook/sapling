/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use land_service_if::LandChangesetRequest;
use scuba_ext::MononokeScubaSampleBuilder;
use scuba_ext::ScubaValue;

use crate::conversion_helpers::convert_hex_to_str;

pub(crate) trait AddScubaRequest: Send + Sync {
    fn add_scuba_params(&self, _scuba: &mut MononokeScubaSampleBuilder) {}
}

impl AddScubaRequest for LandChangesetRequest {
    fn add_scuba_params(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("bookmark_name", self.bookmark.as_str());
        scuba.add(
            "changesets",
            self.changesets
                .iter()
                .map(|changeset| convert_hex_to_str(changeset))
                .collect::<ScubaValue>(),
        );
        scuba.add("repo_name", self.repo_name.as_str());
        scuba.add("number_changesets", self.changesets.len());
    }
}
