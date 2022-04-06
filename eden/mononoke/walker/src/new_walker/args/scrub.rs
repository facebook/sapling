/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use clap::Args;
use fbinit::FacebookInit;
use scuba_ext::MononokeScubaSampleBuilder;
use std::collections::HashSet;
use walker_commands_impl::graph::NodeType;
use walker_commands_impl::pack::PackInfoLogOptions;

use crate::args::parse_node_types;

#[derive(Args, Debug)]
pub struct ScrubOutputNodeArgs {
    /// Node types not to output in debug stdout
    #[clap(long, short = 'O')]
    pub exclude_output_node_type: Vec<String>,
    /// Node types to output in debug stdout
    #[clap(long, short = 'o')]
    pub include_output_node_type: Vec<String>,
}

impl ScrubOutputNodeArgs {
    pub fn parse_args(&self) -> Result<HashSet<NodeType>, Error> {
        parse_node_types(
            self.include_output_node_type.iter(),
            self.exclude_output_node_type.iter(),
            &[],
        )
    }
}

#[derive(Args, Debug)]
pub struct ScrubPackLogArgs {
    /// Node types not to log pack info for
    #[clap(long, short = 'A')]
    pub exclude_pack_log_node_type: Vec<String>,
    /// Node types to log pack info for
    #[clap(long, short = 'a')]
    pub include_pack_log_node_type: Vec<String>,
    /// Scuba table for logging pack info data to. e.g. mononoke_packinfo
    #[clap(long, requires = "include-pack-log-node-type")]
    pub pack_log_scuba_table: Option<String>,
    /// A log file to write Scuba pack info logs to (primarily useful in testing)
    #[clap(long, requires = "include-pack-log-node-type")]
    pub pack_log_scuba_file: Option<String>,
}

impl ScrubPackLogArgs {
    pub fn parse_args(&self, fb: FacebookInit) -> Result<Option<PackInfoLogOptions>, Error> {
        let log_node_types = parse_node_types(
            self.include_pack_log_node_type.iter(),
            self.exclude_pack_log_node_type.iter(),
            &[],
        )?;

        if !log_node_types.is_empty() {
            let mut scuba_builder =
                MononokeScubaSampleBuilder::with_opt_table(fb, self.pack_log_scuba_table.clone());
            if let Some(scuba_log_file) = &self.pack_log_scuba_file {
                scuba_builder = scuba_builder.with_log_file(scuba_log_file)?;
            }

            return Ok(Some(PackInfoLogOptions {
                log_node_types,
                log_dest: scuba_builder,
            }));
        }

        Ok(None)
    }
}
