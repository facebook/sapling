/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Adding redirections to a checkout. This lives outside `edenfs-core` only
//! because of the fbcode-build telemetry sample below; hoist that to the
//! command layer if this path needs to move into the fbinit-free layer.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use hg_util::path::absolute;
use pathdiff::diff_paths;

use crate::checkout::CheckoutConfig;
use crate::checkout::EdenFsCheckout;
use crate::instance::EdenFsInstance;
use crate::redirect::Redirection;
use crate::redirect::RedirectionState;
use crate::redirect::RedirectionType;
use crate::redirect::USER_REDIRECTION_SOURCE;
use crate::redirect::get_configured_redirections;
use crate::redirect::get_effective_redirections;

/// We should return success early iff:
/// 1) we're adding a symlink redirection
/// 2) the symlink already exists
/// 3) the symlink is already a redirection that's managed by EdenFS
fn _should_return_success_early(
    redir_type: RedirectionType,
    configured_redirections: &BTreeMap<PathBuf, Redirection>,
    checkout_path: &Path,
    repo_path: &Path,
) -> Result<bool> {
    if redir_type == RedirectionType::Symlink {
        // We cannot use resolve_repo_relative_path() because it will essentially
        // attempt to resolve any existing symlinks twice. This causes us to never
        // return the correct path for existing symlinks. Instead, we skip resolving
        // and simply check if the absolute path is relative to the checkout path
        // and if any relative paths are pre-existing configured redirections.
        let mut relative_path = repo_path.to_owned();
        if repo_path.is_absolute() {
            let canonical_repo_path = repo_path;
            if !canonical_repo_path.starts_with(checkout_path) {
                return Err(EdenFsError::Other(anyhow!(
                    "The redirection path `{}` doesn't resolve \
                    to a path inside the repo `{}`",
                    repo_path.display(),
                    checkout_path.display()
                )));
            }
            relative_path = diff_paths(canonical_repo_path, checkout_path).unwrap_or_default();
        }
        if let Some(redir) = configured_redirections.get(&relative_path) {
            // A configured symlink is not necessarily effective: eden stop/rm
            // and `eden redirect unmount` delete the symlink from disk while
            // leaving it configured, and add must repair it.
            return Ok(redir.redir_type == RedirectionType::Symlink
                && redir.repo_path == relative_path
                && checkout_path.join(&relative_path).is_symlink());
        }
    }
    Ok(false)
}

/// Given a path, verify that it is an appropriate repo-root-relative path
/// and return the resolved form of that path.
///
/// The ideal is that they pass in `foo` and we return `foo`, but we also
/// allow for the path to be absolute path to `foo`, in which case we resolve
/// it and verify that it falls with the repo and then return the relative
/// path to `foo`.
fn resolve_repo_relative_path(checkout: &EdenFsCheckout, repo_rel_path: &Path) -> Result<PathBuf> {
    let checkout_path = checkout.path();
    if repo_rel_path.is_absolute() {
        // Well, the original intent was to only interpret paths as relative
        // to the repo root, but it's a bit burdensome to require the caller
        // to correctly relativize for that case, so we'll allow an absolute
        // path to be specified.
        if repo_rel_path.starts_with(&checkout_path) {
            let canonical_path = absolute(repo_rel_path).from_err().with_context(|| {
                format!(
                    "Failed to find absolute and normalized path: {}",
                    repo_rel_path.display()
                )
            })?;
            let rel_path = diff_paths(&canonical_path, &checkout_path).with_context(|| {
                format!(
                    "{} starts with {}, but we failed to compute the relative repo path.",
                    canonical_path.display(),
                    checkout_path.display(),
                )
            })?;
            return Ok(rel_path);
        } else {
            return Err(EdenFsError::Other(anyhow!(
                "The path `{}` doesn't resolve to a path inside the repo `{}`",
                repo_rel_path.display(),
                checkout_path.display()
            )));
        };
    }

    // Otherwise, the path must be interpreted as being relative to the repo
    // root, so let's resolve that and verify that it lies within the repo
    let candidate_path = checkout_path.join(repo_rel_path);
    let candidate = absolute(&candidate_path).from_err().with_context(|| {
        format!(
            "Failed to get absolute path for {}",
            candidate_path.display()
        )
    })?;

    if !candidate.starts_with(&checkout_path) {
        return Err(EdenFsError::Other(anyhow!(
            "The redirection path `{}` (canonical path: `{}`) doesn't resolve \
            to a path inside the repo `{}`",
            repo_rel_path.display(),
            candidate.display(),
            checkout_path.display()
        )));
    }
    let relative_path = diff_paths(candidate, &checkout_path).unwrap_or_default();

    // If the resolved and relativized path doesn't match the user-specified
    // path then it means that they either used `..` or a path that resolved
    // through a symlink.  The former is ambiguous, especially because it likely
    // implies that the user is assuming that the path is current working directory
    // relative instead of repo root relative, and the latter is problematic for
    // all of the usual symlink reasons.
    if relative_path != repo_rel_path {
        Err(EdenFsError::Other(anyhow!(
            "The redirection path `{}` resolves to `{}` but must be a canonical \
            repo-root-relative path. Specify either a canonical absolute path \
            to the redirection, or a canonical (without `..` components) path \
            relative to the repository root at `{}`.",
            repo_rel_path.display(),
            relative_path.display(),
            checkout_path.display(),
        )))
    } else {
        Ok(repo_rel_path.to_owned())
    }
}

fn redirection_needs_repair(state: &RedirectionState) -> bool {
    matches!(
        state,
        RedirectionState::NotMounted
            | RedirectionState::SymlinkMissing
            | RedirectionState::SymlinkIncorrect
    )
}

pub async fn try_add_redirection(
    instance: &EdenFsInstance,
    checkout: &EdenFsCheckout,
    config_dir: &Path,
    repo_path: &Path,
    redir_type: RedirectionType,
    force_remount_bind_mounts: bool,
    strict: bool,
    force: bool,
) -> Result<i32> {
    // Get only the explicitly configured entries for the purposes of the
    // add command, so that we avoid writing out any of the effective list
    // of redirections to the local configuration.  That doesn't matter so
    // much at this stage, but when we add loading in profile(s) later we
    // don't want to scoop those up and write them out to this branch of
    // the configuration.
    let mut configured_redirs = get_configured_redirections(checkout).with_context(|| {
        format!(
            "Failed to get configured redirections for checkout {}",
            checkout.path().display()
        )
    })?;

    // We are only checking for pre-existing symlinks in this method, so we
    // can use the configured mounts instead of the effective mounts; the
    // check verifies the symlink's on-disk presence itself.
    if _should_return_success_early(redir_type, &configured_redirs, &checkout.path(), repo_path)? {
        eprintln!("EdenFS managed symlink redirection already exists.");
        return Ok(0);
    }

    // We need to query the status of the mounts to catch things like
    // a redirect being configured but unmounted.  This improves the
    // UX in the case where eg: buck is adding a redirect.  Without this
    // we'd hit the skip case below because it is configured, but we wouldn't
    // bring the redirection back online.
    // However, we keep this separate from the `redirs` list below for
    // the reasons stated in the comment above.
    let effective_redirs = get_effective_redirections(instance, checkout).with_context(|| {
        format!(
            "Failed to get effective redirections for checkout {}",
            checkout.path().display()
        )
    })?;

    let resolved_repo_path =
        resolve_repo_relative_path(checkout, repo_path).with_context(|| {
            format!(
                "Failed to resolve repo relative path for '{}' in checkout {}",
                repo_path.display(),
                checkout.path().display()
            )
        })?;

    let mut redir = Redirection {
        repo_path: resolved_repo_path.clone(),
        redir_type,
        target: None,
        source: USER_REDIRECTION_SOURCE.to_string(),
        state: RedirectionState::MatchesConfiguration,
    };

    if let Some(existing_redir) = effective_redirs.get(&resolved_repo_path) {
        let existing_redir_state = &existing_redir.state;
        if existing_redir.repo_path == redir.repo_path
            && !force_remount_bind_mounts
            && !redirection_needs_repair(existing_redir_state)
        {
            eprintln!(
                "Skipping {}; it is already configured. (use \
                    --force-remount-bind-mounts to force reconfiguring this \
                    redirection.",
                resolved_repo_path.display(),
            );
            return Ok(0);
        }
    }
    // We should prevent users from accidentally overwriting existing
    // directories. We only need to check this condition for bind mounts
    // because symlinks should already fail if the target dir exists.
    if redir_type == RedirectionType::Bind && redir.repo_path().is_dir() {
        if !strict {
            eprintln!(
                "WARNING: {} already exists.\nMounting over \
                an existing directory will overwrite its contents.\nYou can \
                use --strict to prevent overwriting existing directories.\n",
                redir.repo_path.display()
            );
            #[cfg(fbcode_build)]
            {
                let sample = edenfs_telemetry::redirect::build(
                    &redir.repo_path.to_string_lossy(),
                    &checkout.path().to_string_lossy(),
                );
                edenfs_telemetry::send_edenfs_event(sample);
            }
        } else {
            eprintln!(
                "Not adding redirection {} because \
                the --strict option was used.\nIf you would like \
                to add this redirection (not recommended), then \
                rerun this command without --strict.",
                redir.repo_path.display()
            );
            return Ok(1);
        }
    }

    redir
        .apply(instance, checkout, force, "add")
        .await
        .with_context(|| {
            format!(
                "Failed to apply redirection '{}' for checkout {}",
                redir.repo_path.display(),
                checkout.path().display()
            )
        })?;

    // If apply() was successful, we can expect that the `expand_target_abspath`
    // was successful and not handling the `Err` case.
    // Setting target here to make it part of eden checkout config.
    redir.target = redir
        .expand_target_abspath(instance, checkout)
        .ok()
        .flatten();

    // We expressly allow replacing an existing configuration in order to
    // support a user with a local ad-hoc override for global- or profile-
    // specified configuration.
    configured_redirs.insert(repo_path.to_owned(), redir);
    let mut checkout_config = CheckoutConfig::parse_config(config_dir).with_context(|| {
        format!(
            "Failed to parse checkout config using config dir {}",
            config_dir.display()
        )
    })?;
    // and persist the configuration so that we can re-apply it in a subsequent
    // call to `edenfsctl redirect fixup`
    checkout_config
        .update_redirections(config_dir, &configured_redirs)
        .with_context(|| {
            format!(
                "Failed to update redirections for checkout {}",
                checkout.path().display()
            )
        })?;

    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::redirection_needs_repair;
    use crate::redirect::RedirectionState;

    #[test]
    fn test_broken_redirection_states_need_repair() {
        assert!(redirection_needs_repair(&RedirectionState::NotMounted));
        assert!(redirection_needs_repair(&RedirectionState::SymlinkMissing));
        assert!(redirection_needs_repair(
            &RedirectionState::SymlinkIncorrect
        ));
        assert!(!redirection_needs_repair(
            &RedirectionState::MatchesConfiguration
        ));
        assert!(!redirection_needs_repair(&RedirectionState::UnknownMount));
    }
}
