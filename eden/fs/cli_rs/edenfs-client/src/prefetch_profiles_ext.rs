/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Prefetch-profile requests for [`EdenFsCheckout`]. This lives outside
//! `edenfs-core` because it drives thrift prefetch endpoints through the
//! full [`EdenFsClient`](crate::client::EdenFsClient).

use std::collections::HashSet;
use std::env;
use std::process::Command;

use anyhow::Context;
use anyhow::anyhow;
use async_trait::async_trait;
use edenfs_core::checkout::EdenFsCheckout;
use edenfs_core::checkout::PrefetchProfilesResult;
use edenfs_core::checkout::find_checkout;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use thrift_types::edenfs::PrefetchParams;

use crate::client::Client;
use crate::instance::EdenFsInstance;
use crate::methods::EdenThriftMethod;

#[async_trait]
pub trait CheckoutPrefetchExt {
    /// Function to actually cause the prefetch, can be called on a background
    /// process or in the main process.
    /// Only print here if silent is False, as that could send messages
    /// randomly to stdout.
    async fn make_prefetch_request(
        &self,
        instance: &EdenFsInstance,
        all_profile_contents: HashSet<String>,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
        background: bool,
    ) -> Result<()>;

    async fn prefetch_profiles(
        &self,
        instance: &EdenFsInstance,
        profiles: &[String],
        background: bool,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
    ) -> Result<PrefetchProfilesResult>;
}

#[async_trait]
impl CheckoutPrefetchExt for EdenFsCheckout {
    async fn make_prefetch_request(
        &self,
        instance: &EdenFsInstance,
        all_profile_contents: HashSet<String>,
        directories_only: bool,
        _silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
        background: bool,
    ) -> Result<()> {
        let checkout_path = self.path();
        let mut commit_vec = vec![];
        if predict_revisions {
            // The arc and hg commands need to be run in the mount mount, so we need
            // to change the working path if it is not within the mount.
            let cwd = env::current_dir().context("Unable to get current working directory")?;
            let mut changed_dir = false;
            if find_checkout(instance, &cwd).is_err() {
                println!("Setting the current working directory");
                env::set_current_dir(&checkout_path).with_context(|| {
                    anyhow!(
                        "failed to change working directory to '{}'",
                        checkout_path.display()
                    )
                })?;
                changed_dir = true;
            }

            let output = Command::new("arc")
                .arg("stable")
                .arg("best")
                .arg("--verbose")
                .arg("error")
                .output()
                .with_context(|| {
                    anyhow!("Failed to execute subprocess `arc stable best --verbose error`")
                })?;
            if !output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "Unable to predict commits to prefetch, error finding bookmark \
            to prefetch: {}",
                    String::from_utf8_lossy(output.stderr.as_slice())
                )));
            }

            let bookmark = String::from_utf8_lossy(output.stdout.as_slice());
            let bookmark = bookmark.trim();

            let output = Command::new("hg")
                .arg("log")
                .arg("-r")
                .arg(bookmark)
                .arg("-T")
                .arg("{node}")
                .output()
                .with_context(|| {
                    anyhow!("Failed to execute subprocess `hg log -r {bookmark} -T {{node}}`")
                })?;

            if !output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "Unable to predict commits to prefetch, error converting \
                bookmark to commit: {}",
                    String::from_utf8_lossy(output.stderr.as_slice())
                )));
            }

            // If we changed directories to run the subcommands, we should switch
            // back to our previous location
            if changed_dir {
                env::set_current_dir(&cwd)
                    .context("failed to change back to old working directory")?;
            }

            let commit = String::from_utf8_lossy(output.stdout.as_slice());
            let commit = commit.trim().as_bytes().to_vec();
            commit_vec.push(commit);
        }

        if let Some(revs) = revisions {
            for rev in revs {
                let commit = rev.trim().as_bytes().to_vec();
                commit_vec.push(commit);
            }
        }

        let client = instance.get_client();

        let mnt_pt = checkout_path
            .to_str()
            .context("failed to get mount point as str")?
            .as_bytes()
            .to_vec();
        let profile_set = all_profile_contents.into_iter().collect::<Vec<_>>();
        let prefetch_params = PrefetchParams {
            mountPoint: mnt_pt,
            globs: profile_set,
            directoriesOnly: directories_only,
            revisions: commit_vec,
            background,
            returnPrefetchedFiles: false,
            ..Default::default()
        };
        let res = client
            .with_thrift(|thrift| {
                (
                    thrift.prefetchFilesV2(&prefetch_params),
                    EdenThriftMethod::PrefetchFilesV2,
                )
            })
            .await;

        match res {
            Ok(_) => Ok(()),
            Err(err) => Err(EdenFsError::Other(err.into())),
        }
    }

    async fn prefetch_profiles(
        &self,
        instance: &EdenFsInstance,
        profiles: &[String],
        background: bool,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
    ) -> Result<PrefetchProfilesResult> {
        let mut profiles_to_fetch = profiles.to_owned();

        let config = instance
            .get_config()
            .context("unable to load configuration")?;

        if !EdenFsCheckout::should_prefetch_profiles(&config) {
            let reason = "Skipping Prefetch Profiles fetch due to global kill switch. \
                    This means prefetch-profiles.prefetching-enabled is not set in \
                    the EdenFS configs."
                .to_string();
            return Ok(PrefetchProfilesResult::Skipped(reason));
        }

        let mut profile_contents = HashSet::new();

        // special trees prefetch profile which fetches all of the trees in the repo, kick this
        // off before activating the rest of the prefetch profiles
        let tree_profile = "trees";
        // special trees-mobile prefetch profile which fetches a subset of trees in fbsource, kick this
        // off only if not fetching the overarching trees profile, and before activating the rest of the prefetch profiles
        let tree_mobile_profile = "trees-mobile";

        let mut trees_profile_set = HashSet::new();

        // Check for trees first, if it exists, then kick off the prefetch request.
        if profiles_to_fetch.iter().any(|x| x == tree_profile) {
            profiles_to_fetch.retain(|x| *x != *tree_profile);
            // also remove the trees-mobile profile if it exists, but don't fetch it because it is a subset of trees
            profiles_to_fetch.retain(|x| *x != *tree_mobile_profile);

            trees_profile_set.insert("**/*".to_owned());
        } else if profiles_to_fetch.iter().any(|x| x == tree_mobile_profile) {
            profiles_to_fetch.retain(|x| *x != *tree_mobile_profile);

            trees_profile_set.insert("arvr/**/*".to_owned());
            trees_profile_set.insert("fbandroid/**/*".to_owned());
            trees_profile_set.insert("fbcode/**/*".to_owned());
            trees_profile_set.insert("fbobjc/**/*".to_owned());
            trees_profile_set.insert("third-party/**/*".to_owned());
            trees_profile_set.insert("tools/**/*".to_owned());
            trees_profile_set.insert("xplat/**/*".to_owned());
            trees_profile_set.insert("whatsapp/**/*".to_owned());
        }

        if !trees_profile_set.is_empty() {
            self.make_prefetch_request(
                instance,
                trees_profile_set,
                true, // only prefetch directories
                silent,
                revisions.clone(),
                predict_revisions,
                background,
            )
            .await
            .with_context(|| anyhow!("make_prefetch_request() failed, returning early"))?;
            if profiles_to_fetch.is_empty() {
                return Ok(PrefetchProfilesResult::Prefetched);
            }
        }

        for profile in profiles_to_fetch {
            let res = self
                .get_contents_for_profile(&profile, silent)
                .with_context(|| anyhow!("failed to get contents of prefetch profile {profile}"))?;
            profile_contents.extend(res);
        }
        self.make_prefetch_request(
            instance,
            profile_contents,
            directories_only,
            silent,
            revisions,
            predict_revisions,
            background,
        )
        .await
        .with_context(|| anyhow!("make_prefetch_request() failed, returning early"))?;
        Ok(PrefetchProfilesResult::Prefetched)
    }
}
