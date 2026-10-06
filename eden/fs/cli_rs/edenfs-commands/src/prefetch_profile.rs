/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl prefetch-profile

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::str;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::checkout::CheckoutConfig;
use edenfs_client::checkout::CheckoutPrefetchExt;
use edenfs_client::checkout::PrefetchProfilesResult;
use edenfs_client::checkout::find_checkout;
use edenfs_client::utils::expand_path_or_cwd;
use hg_util::path::expand_path;

use crate::ExitCode;
use crate::Subcommand;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
pub struct ActivationOptions {
    #[clap(short, long, help = "Print extra info and warnings.")]
    verbose: bool,
    #[clap(
        long,
        value_parser = |s: &str| -> Result<PathBuf, anyhow::Error> { expand_path_or_cwd(s) },
        default_value = "",
        help = "The checkout for which you want to activate this profile"
    )]
    checkout: PathBuf,
}

#[derive(Parser, Debug)]
pub struct FetchOptions {
    #[clap(flatten)]
    options: ActivationOptions,
    #[clap(
        short,
        long,
        help = "Do not prefetch files, only fetch corresponding directories. \
    This will still activate the profile when running `activate`."
    )]
    directories_only: bool,
    #[clap(
        short,
        long,
        help = "Run the prefetch in the main thread rather than in the \
    background. Normally this command will return once the prefetch \
    has been kicked off, but when this flag is used it to block until \
    all of the files are prefetched."
    )]
    foreground: bool,
    #[clap(
        long,
        num_args = 1..,
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
#[clap(
    about = "Create, manage, and use Prefetch Profiles. Prefetch profiles \
    describe lists of files that Eden will preemptively fetch when directed \
    to do so. Fetching files in advance helps warm caches and leads to faster \
    file operations. This command is primarily for use in automation."
)]
pub enum PrefetchProfileCmd {
    #[clap(about = "Stop recording fetched file paths and save previously \
        collected fetched file paths in the output prefetch profile")]
    Finish {
        #[clap(
            long,
            value_parser = |s: &str| -> Result<PathBuf, std::convert::Infallible> { Ok(expand_path(s)) },
            default_value = "prefetch_profile.txt",
            help = "The output path to store the prefetch profile"
        )]
        output_path: PathBuf,
    },
    #[clap(about = "Start recording fetched file paths.")]
    Record,
    #[clap(about = "List all of the activated prefetch profiles for a checkout.")]
    List {
        #[clap(
            long,
            value_parser = |s: &str| -> Result<PathBuf, anyhow::Error> { expand_path_or_cwd(s) },
            default_value = "",
            help = "The checkout for which you want to see all the profiles"
        )]
        checkout: PathBuf,
        #[clap(long, help = "Output in json rather than human readable text")]
        json: bool,
    },
    #[clap(about = "Adds an entry to the list of profiles to be fetched \
        whenever the `prefetch-profile fetch` subcommand is invoked without \
        additional arguments.")]
    Activate {
        #[clap(flatten)]
        options: ActivationOptions,
        #[clap(help = "Profile to activate.")]
        profile_name: String,
    },
    #[clap(
        about = "Tell EdenFS to STOP smart prefetching the files specified by \
        the prefetch profile."
    )]
    Deactivate {
        #[clap(flatten)]
        options: ActivationOptions,
        #[clap(help = "Profile to deactivate.")]
        profile_name: String,
    },
    #[clap(
        about = "Prefetch all files for the specified prefetch profiles. \
        If no profiles are provided, prefetches all files for all activated \
        proifles instead. This is intended for use after checkout and pull.",
        after_help = "NOTE: When providing both --commits and a list of \
        profiles, you must separate these two lists with `--`. For example: \
        \n\neden prefetch-profile fetch --commits ff77d28f9 dd76d27fa -- \
        eden arc_focus_large trees \n\
        eden prefetch-profile fetch eden arc_focus_large trees \n\
        eden prefetch-profile fetch --commits ff77d28f9"
    )]
    Fetch {
        #[clap(flatten)]
        options: FetchOptions,
        #[clap(
            required = false,
            help = "Fetch only these named profiles instead of the active set of profiles."
        )]
        profile_names: Vec<String>,
        #[clap(long, help = "Output in json rather than human readable text")]
        json: bool,
    },
}

impl PrefetchProfileCmd {
    async fn finish(&self, output_path: &PathBuf) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        let files = client.stop_recording_backing_store_fetch().await?;
        let fetched_files = files
            .fetched_file_paths
            .get("SaplingBackingStore")
            .ok_or_else(|| anyhow!("no Path vector found"))?;
        let mut out_file = File::create(output_path).context("unable to create output file")?;
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

    async fn record(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        client.start_recording_backing_store_fetch().await?;
        Ok(0)
    }

    pub fn print_prefetch_profiles(&self, profiles: Option<&Vec<String>>) -> Result<()> {
        match profiles {
            Some(profiles) if !profiles.is_empty() => {
                for s in profiles.iter() {
                    println!("{s}");
                }
            }
            _ => println!("No active prefetch profiles."),
        };
        Ok(())
    }

    fn print_prefetch_profiles_json(&self, profiles: Option<&Vec<String>>) -> Result<()> {
        let out = match profiles {
            Some(profiles) if !profiles.is_empty() => serde_json::to_string(profiles)
                .context("Failed to serialize list of active prfetch profiles as JSON")?,
            _ => "[]".to_owned(),
        };
        println!("{out}");
        Ok(())
    }

    async fn list(&self, checkout: &Path, json: bool) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client_name = instance.client_name(checkout).with_context(|| {
            anyhow!(
                "Failed to get client name for checkout {}",
                checkout.display()
            )
        })?;
        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(&config_dir);
        match checkout_config {
            Ok(checkout_config) => {
                let profiles = checkout_config.get_prefetch_profiles().ok();
                if json {
                    self.print_prefetch_profiles_json(profiles)?;
                } else {
                    self.print_prefetch_profiles(profiles)?;
                }
                Ok(0)
            }
            Err(_) => Err(anyhow!(
                "Could not print prefetch profile data for {client_name}"
            )),
        }
    }

    async fn activate(&self, options: &ActivationOptions, profile_name: &str) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client_name = instance.client_name(&options.checkout).with_context(|| {
            anyhow!(
                "Failed to get client name for checkout {}",
                options.checkout.display()
            )
        })?;

        #[cfg(fbcode_build)]
        let mut sample =
            edenfs_telemetry::prefetch_profile::activate_event(profile_name, client_name.as_str());

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(&config_dir);
        let result = checkout_config
            .and_then(|mut config| config.activate_profile(profile_name, config_dir));
        match result {
            Ok(_) => {
                #[cfg(fbcode_build)]
                crate::send_edenfs_event(sample);
            }
            Err(e) => {
                #[cfg(fbcode_build)]
                {
                    sample.fail(&e.to_string());
                    crate::send_edenfs_event(sample);
                }
                return Err(anyhow::Error::new(e));
            }
        };

        Ok(0)
    }

    async fn deactivate(
        &self,
        options: &ActivationOptions,
        profile_name: &str,
    ) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client_name = instance.client_name(&options.checkout).with_context(|| {
            anyhow!(
                "Failed to get client name for checkout {}",
                options.checkout.display()
            )
        })?;

        #[cfg(fbcode_build)]
        let mut sample = edenfs_telemetry::prefetch_profile::deactivate_event(
            profile_name,
            client_name.as_str(),
        );

        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(&config_dir);
        let result = checkout_config
            .and_then(|mut config| config.deactivate_profile(profile_name, config_dir));
        if let Err(e) = result {
            #[cfg(fbcode_build)]
            {
                sample.fail(&e.to_string());
                crate::send_edenfs_event(sample);
            }
            return Err(anyhow::Error::new(e));
        }
        #[cfg(fbcode_build)]
        crate::send_edenfs_event(sample);
        Ok(0)
    }

    async fn fetch(
        &self,
        profile_names: &Vec<String>,
        options: &FetchOptions,
        json: bool,
    ) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let checkout_path = &options.options.checkout;
        let client_name = instance.client_name(checkout_path).with_context(|| {
            anyhow!(
                "Failed to get client name for checkout {}",
                checkout_path.display()
            )
        })?;
        let config_dir = instance.config_directory(&client_name);
        let checkout_config = CheckoutConfig::parse_config(&config_dir).with_context(|| {
            anyhow!(
                "Failed to parse config located in config_dir: {}",
                config_dir.display()
            )
        })?;
        let profiles_to_prefetch = if profile_names.is_empty() {
            match checkout_config.get_prefetch_profiles() {
                Ok(res) if !res.is_empty() => res,
                _ => {
                    if json {
                        println!("[]");
                    } else {
                        println!("No profiles to fetch: active profile set is empty.")
                    }
                    return Ok(0);
                }
            }
        } else {
            profile_names
        };

        let directories_only = options.directories_only;

        let checkout = find_checkout(instance, checkout_path).with_context(|| {
            anyhow!(
                "Failed to find checkout with path {}",
                checkout_path.display()
            )
        })?;

        if json {
            println!("{}", serde_json::to_string(&profiles_to_prefetch)?);
        } else {
            let fetched_text = if directories_only {
                "directories"
            } else {
                "files and directories"
            };
            println!(
                "Fetching {} for the following profiles:\n\n  - {}",
                fetched_text,
                profiles_to_prefetch.join("\n  - ")
            );
        }

        let result = checkout
            .prefetch_profiles(
                instance,
                profiles_to_prefetch,
                !options.foreground,
                directories_only,
                !options.options.verbose,
                Some(&options.commits),
                options.predict_commits,
            )
            .await?;

        match result {
            PrefetchProfilesResult::Prefetched => Ok(0),
            PrefetchProfilesResult::Skipped(reason) => {
                eprintln!("{reason}");
                Ok(0)
            }
        }
    }
}

#[async_trait]
impl Subcommand for PrefetchProfileCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            Self::Finish { output_path } => self.finish(output_path).await,
            Self::Record {} => self.record().await,
            Self::List { checkout, json } => self.list(checkout, *json).await,
            Self::Activate {
                options,
                profile_name,
            } => self.activate(options, profile_name).await,
            Self::Deactivate {
                options,
                profile_name,
            } => self.deactivate(options, profile_name).await,
            Self::Fetch {
                profile_names,
                options,
                json,
            } => self.fetch(profile_names, options, *json).await,
        }
    }
}
