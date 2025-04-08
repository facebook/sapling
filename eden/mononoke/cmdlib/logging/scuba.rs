/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::PathBuf;

use anyhow::Error;
use anyhow::Result;
use arg_extensions::ArgDefaults;
use clap::Args;
use fbinit::FacebookInit;
use observability::ObservabilityContext;
use scuba_ext::MononokeScubaSampleBuilder;

/// Command line arguments that control scuba logging
#[derive(Args, Debug)]
pub struct ScubaLoggingArgs {
    /// Set the name of the scuba dataset to log to
    #[clap(long)]
    pub scuba_dataset: Option<String>,
    /// Write scuba logs to a log file
    #[clap(long)]
    pub scuba_log_file: Option<PathBuf>,
    /// Do not use the default scuba dataset for this app
    #[clap(long)]
    pub no_default_scuba_dataset: bool,
    /// Special dataset to be used by warm bookmark cache.  If a binary doesn't
    /// use warm bookmark cache then this parameter is ignored
    #[clap(long)]
    pub warm_bookmark_cache_scuba_dataset: Option<String>,
    /// If true, use the default scuba dataset for warm bookmark cache.
    #[clap(long)]
    pub warm_bookmark_cache_default_scuba_dataset: bool,
}

impl ScubaLoggingArgs {
    pub fn create_scuba_sample_builder(
        &self,
        fb: FacebookInit,
        observability_context: &ObservabilityContext,
        default_scuba_set: &Option<String>,
    ) -> Result<MononokeScubaSampleBuilder> {
        let scuba_logger = if self.scuba_dataset.is_none()
            && self.scuba_log_file.is_none()
            && !self.no_default_scuba_dataset
        {
            // No scuba args specified, so use the default scuba dataset (if any)
            if let Some(default_scuba_dataset) = default_scuba_set {
                MononokeScubaSampleBuilder::new(fb, default_scuba_dataset)?
            } else {
                MononokeScubaSampleBuilder::with_discard()
            }
        } else {
            let mut scuba_logger = if let Some(scuba_dataset) = &self.scuba_dataset {
                MononokeScubaSampleBuilder::new(fb, scuba_dataset.as_str())?
            } else {
                MononokeScubaSampleBuilder::with_discard()
            };
            if let Some(scuba_log_file) = &self.scuba_log_file {
                scuba_logger = scuba_logger.with_log_file(scuba_log_file)?;
            }
            scuba_logger
        };
        let mut scuba_logger = scuba_logger
            .with_observability_context(observability_context.clone())
            .with_seq("seq");

        scuba_logger.add_common_server_data();

        Ok(scuba_logger)
    }

    pub fn create_warm_bookmark_cache_scuba_sample_builder(
        &self,
        fb: FacebookInit,
    ) -> Result<MononokeScubaSampleBuilder, Error> {
        let maybe_scuba = match self.warm_bookmark_cache_scuba_dataset.clone() {
            Some(scuba) => {
                let hostname = hostname::get_hostname()?;
                let sampling_pct = justknobs::get_as::<u64>(
                    "scm/mononoke:warm_bookmark_cache_logging_sampling_pct",
                    None,
                )
                .unwrap_or_default();
                let mut hasher = DefaultHasher::new();
                hostname.hash(&mut hasher);

                if hasher.finish() % 100 < sampling_pct {
                    Some(scuba)
                } else {
                    None
                }
            }
            None => None,
        };

        MononokeScubaSampleBuilder::with_opt_table(fb, maybe_scuba)
    }
}

impl ArgDefaults for ScubaLoggingArgs {
    fn arg_defaults(&self) -> Vec<(&'static str, String)> {
        let mut args = vec![];

        if let Some(scuba_dataset) = &self.scuba_dataset {
            args.push(("scuba_dataset", scuba_dataset.clone().to_string()));
        };
        if self.no_default_scuba_dataset {
            args.push(("no_default_scuba_dataset", String::from("")));
        };

        if let Some(warm_bookmark_cache_scuba_dataset) = &self.warm_bookmark_cache_scuba_dataset {
            args.push((
                "warm_bookmark_cache_scuba_dataset",
                warm_bookmark_cache_scuba_dataset.clone().to_string(),
            ));
        };

        args
    }
}
