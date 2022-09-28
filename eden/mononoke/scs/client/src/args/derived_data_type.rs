/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use source_control::types as thrift;

#[derive(clap::Args, Clone)]
pub(crate) struct DerivedDataTypeArgs {
    #[clap(long, short)]
    /// Type of derived data to derive
    derived_data_type: thrift::DerivedDataType,
}

impl DerivedDataTypeArgs {
    pub fn into_derived_data_type(self) -> thrift::DerivedDataType {
        self.derived_data_type
    }
}
