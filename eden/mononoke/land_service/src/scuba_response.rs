/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use land_service_if::LandChangesetsResponse;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::land_service_impl::convert_hex_to_str;

/// A trait for logging part of a thrift `Response` struct to scuba.
pub(crate) trait AddScubaResponse: Send + Sync {
    fn add_scuba_response(&self, _scuba: &mut MononokeScubaSampleBuilder) {}
}

impl AddScubaResponse for LandChangesetsResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add(
            "new_head",
            convert_hex_to_str(&self.pushrebase_outcome.head),
        );
        scuba.add(
            "pushrebase_distance",
            self.pushrebase_outcome.pushrebase_distance.to_string(),
        );
        scuba.add("retry_num", self.pushrebase_outcome.retry_num.to_string());
        if let Some(old_bookmark_value) = &self.pushrebase_outcome.old_bookmark_value {
            scuba.add("old_bookmark_value", convert_hex_to_str(old_bookmark_value));
        }
    }
}
