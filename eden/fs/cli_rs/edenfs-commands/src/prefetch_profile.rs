/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl prefetch-profile

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use clap::Parser;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::str;
use util::path::expand_path;

use edenfs_client::checkout::find_checkout;
use edenfs_client::checkout::CheckoutConfig;
use edenfs_client::EdenFsInstance;
use edenfs_config::EdenFsConfig;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;

#[cfg(fbcode_build)]
use edenfs_telemetry::prefetch_profile::PrefetchProfileSample;
#[cfg(fbcode_build)]
use edenfs_telemetry::send;
#[cfg(fbcode_build)]
use fbinit::expect_init;

use crate::ExitCode;
use crate::Subcommand;

#[derive(Parser, Debug)]
pub struct CommonOptions {
    // Need default value = False and Store_true
    #[clap(
        short,
        long,
        help = "Print extra info including warnings and the names of the matching files to fetch."
    )]
    verbose: bool,
    #[clap(long, parse(from_str = expand_path), help = "The checkout for which you want to activate this profile")]
    checkout: Option<PathBuf>,
    #[clap(
        short,
        long,
        help = "Do not prefetch profiles only find all the files that match \
    them. This will still list the names of matching files when the \
    verbose flag is also used, and will activate the profile when running \
    `activate`."
    )]
    skip_prefetch: bool,
    #[clap(
        short,
        long,
        help = "Run the prefetch in the main thread rather than in the \
    background. Normally this command will return once the prefetch \
    has been kicked off, but when this flag is used it to block until \
    all of the files are prefetched."
    )]
    foreground: bool,
}

#[derive(Parser, Debug)]
#[clap(name = "prefetch-profile")]
#[clap(about = "Create, manage, and use Prefetch Profiles. This command is \
    primarily for use in automation.")]
pub enum PrefetchCmd {
    #[clap(about = "Stop recording fetched file paths and save previously \
        collected fetched file paths in the output prefetch profile")]
    Finish {
        #[clap(long, parse(from_str = expand_path), help = "The output path to store the prefetch profile")]
        output_path: Option<PathBuf>,
    },
    #[clap(about = "Start recording fetched file paths.")]
    Record,
    #[clap(about = "List all of the currenly activated prefetch profiles for a checkout.")]
    List {
        #[clap(long, parse(from_str = expand_path), help = "The checkout for which you want to see all the profiles")]
        checkout: Option<PathBuf>,
    },
    Activate {
        #[clap(flatten)]
        common: CommonOptions,
        #[clap(help = "Profile to activate.")]
        profile_name: String,
        #[clap(
            long,
            help = "Fetch the profile even if the profile has already been activated"
        )]
        force_fetch: bool,
    },
    Deactivate {
        #[clap(flatten)]
        common: CommonOptions,
        #[clap(help = "Profile to deactivate.")]
        profile_name: String,
    },
    #[clap(about = "Disables prefetch profiles locally")]
    Disable,
    #[clap(about = "Enables prefetch profiles locally")]
    Enable,
}

impl PrefetchCmd {
    async fn finish(
        &self,
        instance: EdenFsInstance,
        output_path: &Option<PathBuf>,
    ) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        let files = client.stopRecordingBackingStoreFetch().await.from_err()?;
        let out_path = match output_path {
            Some(p) => p.clone(),
            None => PathBuf::from(r"prefetch_profile.txt"),
        };
        let fetched_files = files
            .fetchedFilePaths
            .get("HgQueuedBackingStore")
            .ok_or_else(|| anyhow!("no Path vector found"))?;
        let mut out_file = File::create(out_path).context("unable to create output file")?;
        for path_bytes in fetched_files {
            out_file
                .write_all(path_bytes)
                .context("failed to write to output file")?;
            out_file
                .write_all(b"\n")
                .context("failed to write to output file")?;
        }
        Ok(0)
    }

    async fn record(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        client.startRecordingBackingStoreFetch().await.from_err()?;
        Ok(0)
    }

    async fn list(&self, instance: EdenFsInstance, checkout: &Option<PathBuf>) -> Result<ExitCode> {
        let checkout_path = match checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;
        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir);
        match checkout_config {
            Ok(checkout_config) => {
                println!("NAME");
                checkout_config.print_prefetch_profiles();
                Ok(0)
            }
            Err(_) => Err(EdenFsError::Other(anyhow!(
                "Could not print prefetch profile data for {}",
                client_name
            ))),
        }
    }

    async fn activate(
        &self,
        instance: EdenFsInstance,
        options: &CommonOptions,
        profile_name: &str,
        force_fetch: &bool,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };

        let client_name = instance.client_name(&checkout_path)?;

        #[cfg(fbcode_build)]
        let mut sample = {
            let _fb = expect_init();
            let mut sample = PrefetchProfileSample::build(_fb);
            sample.set_activate_event(profile_name, client_name.as_str(), options.skip_prefetch);
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());
        match checkout_config {
            Ok(mut checkout_config) => {
                checkout_config.activate_profile(profile_name, config_dir, force_fetch)?;
                #[cfg(fbcode_build)]
                send(sample.builder);
            }
            #[cfg(fbcode_build)]
            Err(e) => {
                sample.fail(&e.to_string());
                send(sample.builder);
            }
            #[cfg(not(fbcode_build))]
            Err(_) => (),
        }

        if !options.skip_prefetch {
            let checkout = find_checkout(&instance, &checkout_path)?;
            let result_globs = checkout
                .prefetch_profiles(
                    &instance,
                    vec![profile_name.to_string()],
                    !options.foreground,
                    true,
                    !options.verbose,
                    None,
                    false,
                    false,
                    0,
                )
                .await?;
            // there will only every be one commit used to query globFiles here,
            // so no need to list which commit a file is fetched for, it will
            // be the current commit.
            if options.verbose {
                for result in result_globs {
                    for name in result.matchingFiles {
                        println!("{}", String::from_utf8_lossy(&name));
                    }
                }
            }
        }

        Ok(0)
    }

    async fn deactivate(
        &self,
        instance: EdenFsInstance,
        options: &CommonOptions,
        profile_name: &str,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;

        #[cfg(fbcode_build)]
        let mut sample = {
            let _fb = expect_init();
            let mut sample = PrefetchProfileSample::build(_fb);
            sample.set_deactivate_event(profile_name, client_name.as_str());
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());
        match checkout_config {
            Ok(mut checkout_config) => {
                checkout_config.deactivate_profile(profile_name, config_dir)?;
                #[cfg(fbcode_build)]
                send(sample.builder);
                Ok(0)
            }
            Err(e) => {
                #[cfg(fbcode_build)]
                {
                    sample.fail(&e.to_string());
                    send(sample.builder);
                }
                Err(e)
            }
        }
    }

    async fn disable(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let mut loader = EdenFsConfig::loader();
        let home_dir_path = instance
            .get_user_home_dir()
            .ok_or_else(|| {
                EdenFsError::Other(anyhow!(
                    "Failed to disable prefetch-profiles, could not determine user's home directory"
                ))
            })?
            .as_path();
        edenfs_config::load_user(&mut loader, home_dir_path)?;
        let mut edenfs_config = loader.build().map_err(EdenFsError::ConfigurationError)?;
        edenfs_config.set_bool("prefetch-profiles", "prefetching-enabled", false);
        edenfs_config.save_user(home_dir_path)?;
        Ok(0)
    }

    async fn enable(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let mut loader = EdenFsConfig::loader();
        let home_dir_path = instance
            .get_user_home_dir()
            .ok_or_else(|| {
                EdenFsError::Other(anyhow!(
                    "Failed to enable prefetch-profiles, could not determine user's home directory"
                ))
            })?
            .as_path();
        edenfs_config::load_user(&mut loader, home_dir_path)?;
        let mut edenfs_config = loader.build().map_err(EdenFsError::ConfigurationError)?;
        edenfs_config.set_bool("prefetch-profiles", "prefetching-enabled", true);
        edenfs_config.save_user(home_dir_path)?;
        Ok(0)
    }
}

#[async_trait]
impl Subcommand for PrefetchCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        match self {
            Self::Finish { output_path } => self.finish(instance, output_path).await,
            Self::Record {} => self.record(instance).await,
            Self::List { checkout } => self.list(instance, checkout).await,
            Self::Activate {
                common,
                profile_name,
                force_fetch,
            } => {
                self.activate(instance, common, profile_name, force_fetch)
                    .await
            }
            Self::Deactivate {
                common,
                profile_name,
            } => self.deactivate(instance, common, profile_name).await,
            Self::Disable {} => self.disable(instance).await,
            Self::Enable {} => self.enable(instance).await,
        }
    }
}
