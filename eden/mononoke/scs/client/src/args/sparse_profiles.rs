/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use source_control::types as thrift;

#[derive(clap::Args, Clone)]
pub(crate) struct SparseProfilesArgs {
    #[clap(long, short = 'P', value_delimiter = ',')]
    /// Sparse profiles to calculate the size for (defaults to all profiles)
    sparse_profiles: Vec<String>,
}

impl SparseProfilesArgs {
    pub fn into_sparse_profiles(self) -> thrift::SparseProfiles {
        if self.sparse_profiles.is_empty() {
            thrift::SparseProfiles::all_profiles(thrift::AllSparseProfiles {
                ..Default::default()
            })
        } else {
            thrift::SparseProfiles::profiles(self.sparse_profiles)
        }
    }
}
