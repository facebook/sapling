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
pub struct ActivationOptions {
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
pub struct FetchOptions {
    #[clap(flatten)]
    options: ActivationOptions,
    #[clap(
        long,
        multiple_values = true,
        help = "Commit hashes of the commits for which globs should be \
        evaluated. Note that the current commit in the checkout is used \
        if this is not specified. Note that the prefetch profiles are \
        always read from the current commit, not the commits specified \
        here."
    )]
    commits: Vec<String>,
    #[clap(
        long,
        help = "Predict the commits a user is likely to checkout. Evaluate \
        the active prefetch profiles against those commits and fetch the \
        resulting files in those commits. Note that the prefetch profiles \
        are always read from the current commit, not the commits \
        predicted here. This is intended to be used post pull."
    )]
    predict_commits: bool,
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
    #[clap(about = "Tell EdenFS to smart prefetch the files specified by the \
        prefetch profile. (EdenFS will prefetch the files in this profile \
        immediately, when checking out a new commit, and for some pulls).")]
    Activate {
        #[clap(flatten)]
        options: ActivationOptions,
        #[clap(help = "Profile to activate.")]
        profile_name: String,
        #[clap(
            long,
            help = "Fetch the profile even if the profile has already been activated"
        )]
        force_fetch: bool,
    },
    #[clap(hide = true)]
    ActivatePredictive {
        #[clap(flatten)]
        options: ActivationOptions,
        #[clap(
            default_value = "0",
            help = "Optionally set the number of top accessed directories to \
                prefetch, overriding the default."
        )]
        num_dirs: u32,
    },
    #[clap(
        about = "Tell EdenFS to STOP smart prefetching the files specified by the prefetch profile."
    )]
    Deactivate {
        #[clap(flatten)]
        options: ActivationOptions,
        #[clap(help = "Profile to deactivate.")]
        profile_name: String,
    },
    #[clap(hide = true)]
    DeactivatePredictive {
        #[clap(flatten)]
        options: ActivationOptions,
    },
    Fetch {
        #[clap(flatten)]
        options: FetchOptions,
        #[clap(
            required = false,
            help = "Fetch only these named profiles instead of the active set of profiles."
        )]
        profile_names: Vec<String>,
    },
    #[clap(hide = true)]
    FetchPredictive {
        #[clap(flatten)]
        options: FetchOptions,
        #[clap(
            default_value = "0",
            help = "Optionally set the number of top accessed directories to \
                prefetch, overriding the default."
        )]
        num_dirs: u32,
        #[clap(
            long,
            help = "Only run the fetch if activate-predictive has been run. \
                Uses num_dirs set by activate-predictive, or the default."
        )]
        if_active: bool,
    },
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
        options: &ActivationOptions,
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
            let sample = PrefetchProfileSample::activate_event(
                expect_init(),
                profile_name,
                client_name.as_str(),
                options.skip_prefetch,
            );
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());
        let result = checkout_config
            .and_then(|mut config| config.activate_profile(profile_name, config_dir, force_fetch));
        if let Err(e) = result {
            #[cfg(fbcode_build)]
            {
                sample.fail(&e.to_string());
                send(sample.builder);
            }
            return Err(e);
        }
        #[cfg(fbcode_build)]
        send(sample.builder);

        if !options.skip_prefetch {
            let checkout = find_checkout(&instance, &checkout_path)?;
            let result_globs = checkout
                .prefetch_profiles(
                    &instance,
                    &vec![profile_name.to_string()],
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

    async fn activate_predictive(
        &self,
        instance: EdenFsInstance,
        options: &ActivationOptions,
        num_dirs: u32,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };

        let client_name = instance.client_name(&checkout_path)?;

        #[cfg(fbcode_build)]
        let mut sample = {
            let sample = PrefetchProfileSample::activate_predictive_event(
                expect_init(),
                client_name.as_str(),
                options.skip_prefetch,
                num_dirs,
            );
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());

        let result = checkout_config
            .and_then(|mut config| config.activate_predictive_profile(config_dir, num_dirs));
        if let Err(e) = result {
            #[cfg(fbcode_build)]
            {
                sample.fail(&e.to_string());
                send(sample.builder);
            }
            return Err(e);
        }
        #[cfg(fbcode_build)]
        send(sample.builder);

        if !options.skip_prefetch {
            let checkout = find_checkout(&instance, &checkout_path)?;
            let result_globs = checkout
                .prefetch_profiles(
                    &instance,
                    &vec![],
                    !options.foreground,
                    true,
                    !options.verbose,
                    None,
                    false,
                    true,
                    num_dirs,
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
        options: &ActivationOptions,
        profile_name: &str,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;

        #[cfg(fbcode_build)]
        let mut sample = {
            let sample = PrefetchProfileSample::deactivate_event(
                expect_init(),
                profile_name,
                client_name.as_str(),
            );
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());
        let result = checkout_config
            .and_then(|mut config| config.deactivate_profile(profile_name, config_dir));
        if let Err(e) = result {
            #[cfg(fbcode_build)]
            {
                sample.fail(&e.to_string());
                send(sample.builder);
            }
            return Err(e);
        }
        #[cfg(fbcode_build)]
        send(sample.builder);
        Ok(0)
    }

    async fn deactivate_predictive(
        &self,
        instance: EdenFsInstance,
        options: &ActivationOptions,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;

        #[cfg(fbcode_build)]
        let mut sample = {
            let sample = PrefetchProfileSample::deactivate_predictive_event(
                expect_init(),
                client_name.as_str(),
            );
            sample
        };

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone());
        let result =
            checkout_config.and_then(|mut config| config.deactivate_predictive_profile(config_dir));
        if let Err(e) = result {
            #[cfg(fbcode_build)]
            {
                sample.fail(&e.to_string());
                send(sample.builder);
            }
            return Err(e);
        }
        #[cfg(fbcode_build)]
        send(sample.builder);
        Ok(0)
    }

    async fn fetch(
        &self,
        instance: EdenFsInstance,
        profile_names: &Vec<String>,
        options: &FetchOptions,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;
        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone())?;
        let profiles_to_prefetch = if profile_names.is_empty() {
            checkout_config.get_prefetch_profiles()?
        } else {
            profile_names
        };

        if profiles_to_prefetch.is_empty() {
            if options.options.verbose {
                println!("No profiles to fetch")
            }
            return Ok(0);
        }

        let checkout = find_checkout(&instance, &checkout_path)?;
        let result_globs = checkout
            .prefetch_profiles(
                &instance,
                profiles_to_prefetch,
                !options.options.foreground,
                !options.options.skip_prefetch,
                !options.options.verbose,
                Some(&options.commits),
                options.predict_commits,
                false,
                0,
            )
            .await?;
        // there will only every be one commit used to query globFiles here,
        // so no need to list which commit a file is fetched for, it will
        // be the current commit.
        if options.options.verbose {
            for result in result_globs {
                for name in result.matchingFiles {
                    println!("{}", String::from_utf8_lossy(&name));
                }
            }
        }

        Ok(0)
    }

    async fn fetch_predictive(
        &self,
        instance: EdenFsInstance,
        options: &FetchOptions,
        num_dirs: u32,
        if_active: bool,
    ) -> Result<ExitCode> {
        let checkout_path = match &options.options.checkout {
            Some(p) => p.clone(),
            None => env::current_dir().context("Unable to retrieve current working dir")?,
        };
        let client_name = instance.client_name(&checkout_path)?;
        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(config_dir.clone())?;

        if if_active && !checkout_config.predictive_prefetch_is_active() {
            if options.options.verbose {
                println!(
                    "Predictive prefetch profiles have not been activated and \
                    --if-active was specified. Skipping fetch."
                );
            }
            return Ok(0);
        }

        // If num_dirs is given, use the specified num_dirs. If num_dirs is
        // not given (args.num_dirs == 0), predictive fetch with default num
        // dirs unless there is an active num dirs saved in the checkout config
        let predictive_num_dirs = if num_dirs == 0 && checkout_config.get_predictive_num_dirs() != 0
        {
            checkout_config.get_predictive_num_dirs()
        } else {
            0
        };

        let checkout = find_checkout(&instance, &checkout_path)?;
        let result_globs = checkout
            .prefetch_profiles(
                &instance,
                &vec![],
                !options.options.foreground,
                !options.options.skip_prefetch,
                !options.options.verbose,
                Some(&options.commits),
                options.predict_commits,
                true,
                predictive_num_dirs,
            )
            .await?;

        // there will only every be one commit used to query globFiles here,
        // so no need to list which commit a file is fetched for, it will
        // be the current commit.
        if options.options.verbose {
            for result in result_globs {
                for name in result.matchingFiles {
                    println!("{}", String::from_utf8_lossy(&name));
                }
            }
        }

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
                options,
                profile_name,
                force_fetch,
            } => {
                self.activate(instance, options, profile_name, force_fetch)
                    .await
            }
            Self::ActivatePredictive { options, num_dirs } => {
                self.activate_predictive(instance, options, *num_dirs).await
            }
            Self::Deactivate {
                options,
                profile_name,
            } => self.deactivate(instance, options, profile_name).await,
            Self::DeactivatePredictive { options } => {
                self.deactivate_predictive(instance, options).await
            }
            Self::Fetch {
                profile_names,
                options,
            } => self.fetch(instance, profile_names, options).await,
            Self::FetchPredictive {
                options,
                num_dirs,
                if_active,
            } => {
                self.fetch_predictive(instance, options, *num_dirs, *if_active)
                    .await
            }
        }
    }
}
