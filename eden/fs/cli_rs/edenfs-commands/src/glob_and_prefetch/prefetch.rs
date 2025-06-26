/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl prefetch

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_telemetry::collect_system_info;
use edenfs_telemetry::edenfs_events_mapper;
use edenfs_utils::path_from_bytes;

use crate::ExitCode;
use crate::get_edenfs_instance;
use crate::glob_and_prefetch::common::CommonArgs;

#[derive(Parser, Debug)]
#[clap(
    about = "Prefetch content for matching file patterns. Glob patterns can be provided via a pattern file. This command does not do any filtering based on source control state or gitignore files."
)]
pub struct PrefetchCmd {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(
        long,
        help = "DEPRECATED: Do not print the names of the matching files"
    )]
    silent: bool,

    #[clap(long, help = "Do not prefetch files; only prefetch directories")]
    directories_only: bool,

    #[clap(long, help = "Run the prefetch in the background")]
    background: bool,

    #[clap(
        long,
        help = "Print the paths being prefetched. Does not work if using --background"
    )]
    debug_print: bool,
}

impl PrefetchCmd {
    fn new_sample(&self, mount_point: &Path) -> edenfs_telemetry::EdenSample {
        let mut sample = edenfs_telemetry::EdenSample::new();
        collect_system_info(&mut sample, edenfs_events_mapper);
        sample.add_string("logged_by", "cli_rs");
        sample.add_string("type", "prefetch");
        sample.add_string(
            "checkout",
            mount_point
                .file_name()
                .unwrap_or_default()
                .to_str()
                .unwrap_or_default(),
        );
        sample.add_bool("directories_only", self.directories_only);
        sample.add_bool("background", self.background);
        if let Some(pattern_file) = self.common.pattern_file.as_ref() {
            sample.add_string("pattern_file", pattern_file.to_str().unwrap_or_default());
        }
        if !self.common.pattern.is_empty() {
            sample.add_string_list("patterns", self.common.pattern.clone());
        }
        sample
    }
}

#[async_trait]
impl crate::Subcommand for PrefetchCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        let (mount_point, _search_root) = self.common.get_mount_point_and_search_root()?;

        let mut sample = self.new_sample(&mount_point);

        let patterns = self.common.load_patterns()?;
        let silent = self.silent || !self.debug_print;
        let return_prefetched_files = !(self.background || silent);

        let result = match client
            .prefetch_files(
                &mount_point,
                patterns.clone(),
                self.directories_only,
                None,
                None::<PathBuf>,
                Some(self.background),
                None,
                return_prefetched_files,
            )
            .await
        {
            Ok(r) => {
                sample.add_bool("success", true);
                Ok(r)
            }
            Err(e) => {
                sample.add_bool("success", false);
                sample.add_string("error", format!("{:#}", e).as_str());
                Err(e)
            }
        }?;

        // NOTE: Is the really still needed? We should not be falling back at all anymore.
        sample.add_bool("prefetchV2_fallback", false);

        if return_prefetched_files {
            if !patterns.is_empty()
                && result
                    .prefetched_files
                    .as_ref()
                    .is_none_or(|pf| pf.matching_files.is_empty())
            {
                eprint!("No files were matched by the pattern");
                if !patterns.is_empty() {
                    eprint!("s");
                }
                eprintln!(" specified.\nSee `eden prefetch -h` for docs on pattern matching.");
            }

            if let Some(prefetched_files) = &result.prefetched_files {
                sample.add_int(
                    "files_fetched",
                    prefetched_files.matching_files.len() as i64,
                );

                if self.debug_print {
                    for file in &prefetched_files.matching_files {
                        println!("{}", path_from_bytes(file)?.display());
                    }
                }
            }
        }

        edenfs_telemetry::send(edenfs_telemetry::EDEN_EVENTS_SCUBA.to_string(), sample);
        Ok(0)
    }
}
