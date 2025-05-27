/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl redirect

use std::collections::BTreeMap;
#[cfg(target_os = "macos")]
use std::ffi::OsStr;
#[cfg(target_os = "macos")]
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;
#[cfg(target_os = "macos")]
use dialoguer::Confirm;
use edenfs_client::checkout::CheckoutConfig;
use edenfs_client::checkout::find_checkout;
#[cfg(target_os = "macos")]
use edenfs_client::redirect::APFS_HELPER;
use edenfs_client::redirect::REPO_SOURCE;
use edenfs_client::redirect::Redirection;
use edenfs_client::redirect::RedirectionState;
use edenfs_client::redirect::RedirectionType;
use edenfs_client::redirect::get_configured_redirections;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::redirect::get_effective_redirs_for_mount;
use edenfs_client::redirect::try_add_redirection;
use edenfs_client::utils::expand_path_or_cwd;
use edenfs_client::utils::remove_trailing_slash;
use hg_util::path::expand_path;
use tabular::Table;
use tabular::row;

use crate::ExitCode;
use crate::Subcommand;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(name = "redirect")]
#[clap(about = "List and manipulate redirected paths")]
pub enum RedirectCmd {
    #[clap(about = "List redirections")]
    List {
        #[clap(long, help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(long, help = "output in json rather than human readable text")]
        json: bool,
    },
    #[clap(about = "Add or change a redirection")]
    Add {
        #[clap(long, parse(from_str = expand_path), help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(parse(from_str = expand_path), index = 1, help = "The path in the repo which should be redirected")]
        repo_path: PathBuf,
        #[clap(index = 2, help = "The type of the redirection", possible_values = ["bind", "symlink"])]
        redir_type: String,
        #[clap(
            long,
            help = "Unmount and re-bind mount any bind mount redirections to ensure that they are \
            pointing to the right place. This is not the default behavior in the interest of \
            preserving kernel caches."
        )]
        force_remount_bind_mounts: bool,
        #[clap(
            long,
            help = "force the bind mount to fail if it would overwrite a pre-existing directory"
        )]
        strict: bool,
        #[clap(
            long,
            help = "Forcefully add redirection, removing any existing files/directories that are present at the redirection location."
        )]
        force: bool,
    },
    #[clap(
        about = "Unmount all effective redirection configuration, but preserve the configuration \
        so that a subsequent fixup will restore it"
    )]
    Unmount {
        #[clap(long, parse(from_str = expand_path), help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(
            long,
            help = "Forcefully unmount redirection, removing any existing files/directories that are present at the redirection location."
        )]
        force: bool,
    },
    #[clap(about = "Delete a redirection")]
    Del {
        #[clap(long, parse(from_str = expand_path), help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(parse(from_str = expand_path), index = 1, help = "The path in the repo which should no longer be redirected")]
        repo_path: PathBuf,
        #[clap(
            long,
            help = "Forcefully delete redirection, removing any existing files/directories that are present at the redirection location."
        )]
        force: bool,
    },
    #[clap(
        about = "Fixup redirection configuration; redirect things that should be redirected and \
        remove things that should not be redirected"
    )]
    Fixup {
        #[clap(long, parse(from_str = expand_path), help = "The EdenFS mount point path.")]
        mount: Option<PathBuf>,
        #[clap(
            long,
            help = "Unmount and re-bind mount any bind mount redirections to ensure that they are \
            pointing to the right place. This is not the default behavior in the interest of \
            preserving kernel caches"
        )]
        force_remount_bind_mounts: bool,
        #[clap(
            long,
            help = "By default, paths from all sources are fixed. Setting this flag to true will \
            fix paths only from the .eden-redirections source."
        )]
        only_repo_source: bool,
        #[clap(
            long,
            help = "Forcefully fix redirections, removing any existing files/directories that are present at the redirection location."
        )]
        force: bool,
    },
    #[clap(about = "Delete stale apfs volumes")]
    CleanupApfs {},
}

impl RedirectCmd {
    fn print_redirection_table(
        &self,
        redirections: BTreeMap<PathBuf, Redirection>,
    ) -> Result<ExitCode> {
        let mut table = Table::new("{:<}    {:<}    {:<}    {:<}    {:<}");
        table.add_row(row!("REPO_PATH", "TYPE", "TARGET", "SOURCE", "STATE"));
        for redir in redirections.into_values() {
            table.add_row(row!(
                redir.repo_path().display(),
                redir.redir_type,
                redir
                    .target
                    .map(|x| x.display().to_string())
                    .unwrap_or_default(),
                redir.source,
                redir.state,
            ));
        }
        println!("{}", table);
        Ok(0)
    }

    fn print_redirection_json(
        &self,
        redirections: BTreeMap<PathBuf, Redirection>,
    ) -> Result<ExitCode> {
        let json_out = serde_json::to_string(&redirections.into_values().collect::<Vec<_>>())
            .with_context(|| anyhow!("could not serialize redirections",))?;
        println!("{}", json_out);
        Ok(0)
    }

    async fn list(&self, mount: Option<PathBuf>, json: bool) -> Result<ExitCode> {
        let mount = match mount {
            Some(provided) => provided,
            None => expand_path_or_cwd("").with_context(|| {
                anyhow!("could not infer mount: could not determine current working directory")
            })?,
        };
        let instance = get_edenfs_instance();
        let redirections = get_effective_redirs_for_mount(instance, mount)
            .with_context(|| anyhow!("Failed to get redirections for mount"))?;

        if json {
            self.print_redirection_json(redirections)
        } else {
            self.print_redirection_table(redirections)
        }
    }

    async fn add(
        &self,
        mount: Option<PathBuf>,
        repo_path: &Path,
        redir_type: &str,
        force_remount_bind_mounts: bool,
        strict: bool,
        force: bool,
    ) -> Result<ExitCode> {
        let repo_path = remove_trailing_slash(repo_path);
        let redir_type = RedirectionType::from_str(redir_type)?;
        let instance = get_edenfs_instance();
        let mount = match mount {
            Some(provided) => provided,
            None => expand_path_or_cwd("").with_context(|| {
                anyhow!("could not infer mount: could not determine current working directory")
            })?,
        };
        let client_name = instance.client_name(&mount)?;
        let config_dir = instance.config_directory(&client_name);
        let checkout = find_checkout(instance, &mount)?;
        try_add_redirection(
            instance,
            &checkout,
            &config_dir,
            &repo_path,
            redir_type,
            force_remount_bind_mounts,
            strict,
            force,
        )
        .await
        .with_context(|| {
            format!(
                "Could not add redirection {} of type {}",
                &repo_path.display(),
                redir_type,
            )
        })?;
        Ok(0)
    }

    async fn unmount(&self, mount: Option<PathBuf>, force: bool) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let mount = match mount {
            Some(provided) => provided,
            None => expand_path_or_cwd("").with_context(|| {
                anyhow!("could not infer mount: could not determine current working directory")
            })?,
        };
        let checkout = find_checkout(instance, &mount)?;
        let client_name = instance.client_name(&mount)?;
        let config_dir = instance.config_directory(&client_name);
        let mut checkout_config =
            CheckoutConfig::parse_config(config_dir.clone()).with_context(|| {
                format!(
                    "Failed to parse checkout config using config dir {}",
                    &config_dir.display()
                )
            })?;
        // Remove the redirection targets from the config so that proj-fs pre-delete notification does not block deletion on symlink
        // This will be re-created when mounting the checkout again.
        checkout_config
            .remove_redirection_targets(&config_dir)
            .with_context(|| "Failed to remove redirection targets from config")?;
        let redirs = get_effective_redirections(instance, &checkout).with_context(|| {
            anyhow!(
                "Could not get effective redirections for checkout {}",
                checkout.path().display()
            )
        })?;
        for redir in redirs.values() {
            redir
                .remove_existing(instance, &checkout, false, force, "unmount")
                .await
                .with_context(|| {
                    anyhow!(
                        "failed to remove existing redirection {}",
                        redir.repo_path.display()
                    )
                })?;
        }

        // recompute the current state and catch any failures
        let recomputed_redirs =
            get_effective_redirections(instance, &checkout).with_context(|| {
                anyhow!(
                    "Could not get effective redirections for checkout {}",
                    checkout.path().display()
                )
            })?;

        for redir in recomputed_redirs.values() {
            if redir.state == RedirectionState::MatchesConfiguration {
                eprintln!("error: at least one redirection does not match its configuration");
                return Ok(1);
            }
        }
        Ok(0)
    }

    async fn del(&self, mount: Option<PathBuf>, repo_path: &Path, force: bool) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let mount = match mount {
            Some(provided) => provided,
            None => expand_path_or_cwd("").with_context(|| {
                anyhow!("could not infer mount: could not determine current working directory")
            })?,
        };
        let checkout = find_checkout(instance, &mount)?;
        let client_name = instance.client_name(&mount)?;
        let config_dir = instance.config_directory(&client_name);
        let mut redirs = get_configured_redirections(&checkout).with_context(|| {
            anyhow!(
                "Could not get configured redirections for checkout {}",
                checkout.path().display()
            )
        })?;

        // Note that we're deliberately not using the same validation logic
        // for args.repo_path that we do for the add case for now so that we
        // provide a way to remove bogus redirection paths.  After we've deployed
        // the improved `add` validation for a while, we can use it here also.
        if let Some(redir) = redirs.get(repo_path) {
            let mut checkout_config = CheckoutConfig::parse_config(config_dir.clone())
                .with_context(|| {
                    format!(
                        "Failed to parse checkout config using config dir {}",
                        &config_dir.display()
                    )
                })?;
            // Remove the redirection target from the config so that proj-fs pre-delete notification does not block deletion on symlink
            checkout_config
                .remove_redirection_target(&config_dir, repo_path)
                .with_context(|| {
                    format!(
                        "Failed to remove redirection target for {} from config",
                        repo_path.display()
                    )
                })?;
            redir
                .remove_existing(instance, &checkout, false, force, "del")
                .await
                .with_context(|| {
                    format!(
                        "Failed to remove existing redirection {}",
                        repo_path.display()
                    )
                })?;
            redirs.remove(repo_path);
            checkout_config
                .update_redirections(&config_dir, &redirs)
                .with_context(|| {
                    format!(
                        "Failed to update redirections for checkout {}",
                        checkout.path().display()
                    )
                })?;
            return Ok(0);
        }

        let effective_redirs =
            get_effective_redirections(instance, &checkout).with_context(|| {
                anyhow!(
                    "Could not get configured redirections for checkout {}",
                    checkout.path().display()
                )
            })?;
        if let Some(effective_redir) = effective_redirs.get(repo_path) {
            // This path isn't possible to trigger until we add profiles,
            // but let's be ready for it anyway.
            println!(
                "error: {} is defined by {} and cannot be removed using `edenfsctl redirect del {}`",
                repo_path.display(),
                &effective_redir.source,
                repo_path.display()
            );
            return Ok(1);
        }
        println!("{} is not a known redirection", repo_path.display());
        Ok(1)
    }

    async fn fixup(
        &self,
        mount: Option<PathBuf>,
        force_remount_bind_mounts: bool,
        only_repo_source: bool,
        force: bool,
    ) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let mount = match mount {
            Some(provided) => provided,
            None => expand_path_or_cwd("").with_context(|| {
                anyhow!("could not infer mount: could not determine current working directory")
            })?,
        };
        let checkout = find_checkout(instance, &mount)?;
        let redirs = get_effective_redirections(instance, &checkout).with_context(|| {
            anyhow!(
                "Could not get configured redirections for checkout {}",
                checkout.path().display()
            )
        })?;

        for redir in redirs.values() {
            if redir.state == RedirectionState::MatchesConfiguration
                && !(force_remount_bind_mounts && redir.redir_type == RedirectionType::Bind)
            {
                tracing::debug!(
                    ?redir,
                    "not fixing since it's already matching configuration"
                );
                continue;
            }

            if only_repo_source && redir.source != REPO_SOURCE {
                tracing::debug!(?redir, "not fixing due to not from repo source");
                continue;
            }

            eprintln!(
                "Fixing {}, state = {:?}",
                redir.repo_path.display(),
                redir.state
            );

            if redir.redir_type == RedirectionType::Unknown {
                tracing::debug!(?redir, "not fixing due to unknown redirection type");
                continue;
            }

            if let Err(e) = redir.apply(instance, &checkout, force, "fixup").await {
                eprintln!(
                    "Unable to apply redirection `{}`: {}",
                    redir.repo_path.display(),
                    e
                );
            }
        }

        let effective_redirs =
            get_effective_redirections(instance, &checkout).with_context(|| {
                anyhow!(
                    "Failed to get effective redirections for checkout {}",
                    checkout.path().display()
                )
            })?;
        for redir in effective_redirs.values() {
            if redir.state != RedirectionState::MatchesConfiguration {
                // When --only-repo-source is passed, we may fail to fixup some redirections.
                // This scenario is ok and should not be considered a failure.
                if !only_repo_source || redir.source == REPO_SOURCE {
                    return Ok(1);
                }
            }
        }
        Ok(0)
    }

    #[cfg(not(target_os = "macos"))]
    async fn cleanup_apfs(&self) -> Result<ExitCode> {
        Err(anyhow!("Cannot run cleanup-apfs: Unsupported Platform"))
    }

    #[cfg(target_os = "macos")]
    async fn cleanup_apfs(&self) -> Result<ExitCode> {
        match Redirection::have_apfs_helper() {
            Err(e) => return Err(anyhow!("Cannot run cleanup-apfs: {}", e)),
            Ok(res) => {
                if !res {
                    return Err(anyhow!(
                        "Cannot run cleanup-apfs: {} does not exist",
                        APFS_HELPER
                    ));
                }
            }
        }

        let instance = get_edenfs_instance();
        let mounts = instance
            .get_configured_mounts_map()
            .with_context(|| anyhow!("could not get configured mounts map for EdenFS instance"))?;

        let mut args: Vec<&OsStr> = vec!["list-stale-volumes", "--json"]
            .into_iter()
            .map(OsStr::new)
            .collect::<Vec<_>>();
        args.extend(mounts.keys().map(|m| m.as_os_str()));
        let output = Command::new(APFS_HELPER)
            .args(&args)
            .output()
            .with_context(|| {
                format!(
                    "Failed to execute apfs_helper cmd: `{} {:?}`.",
                    APFS_HELPER,
                    args.join(OsStr::new(" ")),
                )
            })?;
        if !output.status.success() {
            return Err(anyhow!(
                "failed to fetch stale volumes. stderr: {}\n stdout: {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ));
        }

        let stale_output = std::str::from_utf8(&output.stdout).with_context(|| {
            anyhow!("Failed to convert list-stale-volumes output to utf8 string")
        })?;
        let stale_json: serde_json::Value = serde_json::from_str(stale_output)
            .with_context(|| anyhow!("Failed to parse list-stale-volumes output as JSON"))?;

        let default_v = vec![];
        let stale_volumes = stale_json.as_array().unwrap_or(&default_v);
        if stale_volumes.is_empty() {
            println!("No stale volumes detected");
            return Ok(0);
        }

        if std::io::stdin().is_terminal() {
            println!("Warning: this operation will permanently delete the following volumes:");
            for volume in stale_volumes.iter() {
                println!("    {}", volume.as_str().unwrap_or(""));
            }

            if !Confirm::new()
                .with_prompt("Do you want to continue?")
                .interact()?
            {
                println!("Not deleting volumes");
                return Ok(2);
            }
        }

        let mut res = 0;
        for vol in stale_volumes {
            if let Some(vol_str) = vol.as_str() {
                let args = &["delete-volume", vol_str];
                let output = Command::new(APFS_HELPER)
                    .args(args)
                    .output()
                    .with_context(|| {
                        format!(
                            "Failed to execute apfs_helper cmd: `{} {}`.",
                            APFS_HELPER,
                            shlex::join(args.iter().copied()),
                        )
                    })?;
                if !output.status.success() {
                    res = 1;
                    eprintln!(
                        "Failed to delete volume {} due to {}",
                        vol_str,
                        String::from_utf8_lossy(&output.stderr)
                    );
                } else {
                    println!("Deleted volume {}", vol_str);
                }
            } else {
                eprintln!(
                    "Could not convert serde_json::Value object to string: {}",
                    vol
                );
            }
        }
        Ok(res)
    }
}

#[async_trait]
impl Subcommand for RedirectCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            Self::List { mount, json } => self.list(mount.to_owned(), *json).await,
            Self::Add {
                mount,
                repo_path,
                redir_type,
                force_remount_bind_mounts,
                strict,
                force,
            } => {
                self.add(
                    mount.to_owned(),
                    repo_path,
                    redir_type,
                    *force_remount_bind_mounts,
                    *strict,
                    *force,
                )
                .await
            }
            Self::Unmount { mount, force } => self.unmount(mount.to_owned(), *force).await,
            Self::Del {
                mount,
                repo_path,
                force,
            } => self.del(mount.to_owned(), repo_path, *force).await,
            Self::Fixup {
                mount,
                force_remount_bind_mounts,
                only_repo_source,
                force,
            } => {
                self.fixup(
                    mount.to_owned(),
                    *force_remount_bind_mounts,
                    *only_repo_source,
                    *force,
                )
                .await
            }
            Self::CleanupApfs {} => self.cleanup_apfs().await,
        }
    }

    fn get_mount_path_override(&self) -> Option<PathBuf> {
        match self {
            Self::List { mount, .. }
            | Self::Add { mount, .. }
            | Self::Unmount { mount, .. }
            | Self::Del { mount, .. }
            | Self::Fixup { mount, .. } => mount.to_owned(),
            Self::CleanupApfs {} => None,
        }
    }
}
