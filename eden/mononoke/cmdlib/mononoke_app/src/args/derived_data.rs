/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::builder::PossibleValuesParser;
use clap::ArgGroup;
use clap::Args;
use metaconfig_types::DerivedDataTypesConfig;
use mononoke_types::DerivableType;
use strum::IntoEnumIterator;

#[derive(Args, Debug)]
pub struct DerivedDataArgs {
    /// Derived data type to use for this command
    #[clap(short = 'T', long, value_parser = PossibleValuesParser::new(DerivableType::iter().map(|t| DerivableType::name(&t))))]
    derived_data_type: String,
}

#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("derived_data_types_group")
        .required(true)
        .args(&["derived_data_types", "all_types"]),
))]
pub struct MultiDerivedDataArgs {
    /// Derived data types to use for this command
    #[clap(long, short = 'T', value_parser = PossibleValuesParser::new(DerivableType::iter().map(|t| DerivableType::name(&t))))]
    derived_data_types: Vec<String>,

    /// Whether all enabled derived data types should be used for this command
    #[clap(long)]
    all_types: bool,
}

impl DerivedDataArgs {
    pub fn resolve_type(&self) -> Result<DerivableType> {
        DerivableType::from_name(&self.derived_data_type)
    }
}

impl MultiDerivedDataArgs {
    pub fn resolve_types(&self, config: &DerivedDataTypesConfig) -> Result<Vec<DerivableType>> {
        if self.all_types {
            Ok(config.types.iter().copied().collect())
        } else {
            self.derived_data_types
                .iter()
                .map(|t| DerivableType::from_name(t))
                .collect()
        }
    }
}
