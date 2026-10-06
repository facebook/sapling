/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt;
#[cfg(unix)]
use std::fs::Permissions;
use std::future::Future;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use edenfs_core::checkout::EdenFsCheckout;
use edenfs_core::checkout::find_checkout;
use edenfs_core::checkout::get_mounts;
use edenfs_core::fsutil::forcefully_remove_dir_all;
use edenfs_core::instance::EdenFsInstance;
use edenfs_core::redirect::Redirection;
use edenfs_core::redirect::RedirectionBackingCleanupAction;
use edenfs_core::redirect::RedirectionBackingCleanupPlanner;
#[cfg(target_os = "linux")]
use edenfs_core::redirect::finish_backing_cleanup_unmount;
use edenfs_core::redirect::get_effective_redirections;
#[cfg(target_os = "linux")]
use edenfs_core::redirect::redirection_mount_status;
use edenfs_utils::is_active_eden_mount;
use fail::fail_point;
use tracing::debug;
use tracing::warn;

const DEFAULT_AUXILIARY_PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

#[cfg(target_os = "macos")]
const BACKING_CLEANUP_RECOVERY: &str = "APFS volumes left behind require manual cleanup.";
#[cfg(not(target_os = "macos"))]
const BACKING_CLEANUP_RECOVERY: &str =
    "Run `eden du --clean-orphaned` to reclaim leftover backing storage.";

/// Whether checkout removal should preserve or delete managed redirection backing targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BackingCleanupPolicy {
    /// Leave all redirection backing targets intact.
    Preserve,
    /// Delete every managed redirection backing target.
    DeleteManaged,
}

/// Options controlling removal of one registered EdenFS checkout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveCheckoutOptions {
    /// Leave the checkout mount-point directory in place after unregistering it.
    pub preserve_mount_point: bool,
    /// Disable forced EdenFS unmounting.
    pub no_force: bool,
    /// Maximum wait for cleanup planning and redirection unmounts.
    pub auxiliary_process_timeout: Duration,
    /// Policy for redirection backing targets.
    pub backing_cleanup: BackingCleanupPolicy,
}

impl Default for RemoveCheckoutOptions {
    fn default() -> Self {
        Self {
            preserve_mount_point: false,
            no_force: false,
            auxiliary_process_timeout: DEFAULT_AUXILIARY_PROCESS_TIMEOUT,
            backing_cleanup: BackingCleanupPolicy::Preserve,
        }
    }
}

/// A non-fatal condition encountered while removing a checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveCheckoutWarning {
    /// Redirection cleanup failed before checkout removal continued.
    RedirectionUnmountFailed { path: PathBuf, error: String },
    /// Redirection cleanup exceeded the configured timeout before checkout removal continued.
    RedirectionUnmountTimedOut { path: PathBuf, timeout: Duration },
    /// Managed backing cleanup was skipped because the checkout's client config is missing.
    BackingCleanupSkipped { path: PathBuf },
    /// Managed backing cleanup skipped repository-configured redirections because the checkout
    /// is not mounted.
    RepositoryRedirectionCleanupSkipped { path: PathBuf },
    /// Some managed backing could not be planned or deleted before checkout removal continued.
    BackingCleanupFailed { path: PathBuf, error: String },
}

/// Progress and non-fatal warnings reported while removing a checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveCheckoutEvent {
    /// A nonempty batch of backing targets is about to be deleted.
    DeletingBackingStorage,
    /// Removal continues after this warning.
    Warning(RemoveCheckoutWarning),
}

impl fmt::Display for RemoveCheckoutWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RedirectionUnmountFailed { path, error } => write!(
                formatter,
                "Error unmounting redirections for {}: {error}",
                path.display()
            ),
            Self::RedirectionUnmountTimedOut { path, timeout } => write!(
                formatter,
                "Unmounting redirections for {} timed out after {} seconds. Continuing with unmount...",
                path.display(),
                timeout.as_secs_f64()
            ),
            Self::BackingCleanupSkipped { path } => write!(
                formatter,
                "Skipping redirection backing cleanup for {} because its checkout config is missing. {BACKING_CLEANUP_RECOVERY}",
                path.display()
            ),
            Self::RepositoryRedirectionCleanupSkipped { path } => write!(
                formatter,
                "Cannot inspect repository-configured redirections while {} is unmounted. Their backing storage may remain. {BACKING_CLEANUP_RECOVERY}",
                path.display()
            ),
            Self::BackingCleanupFailed { path, error } => write!(
                formatter,
                "Redirection backing cleanup did not finish for {}: {error}. Continuing with removal. {BACKING_CLEANUP_RECOVERY}",
                path.display()
            ),
        }
    }
}

/// Remove one registered EdenFS checkout.
pub async fn remove_checkout(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
) -> Result<()> {
    remove_checkout_with_warning_handler(instance, path, options, |warning| warn!("{warning}"))
        .await
}

/// Remove one registered EdenFS checkout and report non-fatal warnings to the caller.
pub async fn remove_checkout_with_warning_handler(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
    mut warning_handler: impl FnMut(RemoveCheckoutWarning),
) -> Result<()> {
    remove_checkout_with_event_handler(instance, path, options, |event| {
        if let RemoveCheckoutEvent::Warning(warning) = event {
            warning_handler(warning);
        }
    })
    .await
}

/// Remove one registered EdenFS checkout and report progress and warnings to the caller.
pub async fn remove_checkout_with_event_handler(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
    mut event_handler: impl FnMut(RemoveCheckoutEvent),
) -> Result<()> {
    let mut warning_handler = |warning| event_handler(RemoveCheckoutEvent::Warning(warning));
    let path = tokio::fs::canonicalize(path)
        .await
        .with_context(|| format!("failed to canonicalize checkout path {}", path.display()))?;
    let client_dir = instance.client_dir_for_mount_point(&path)?;
    let checkout_is_active = is_active_eden_mount(&path);

    let client_config = client_dir.join("config.toml");
    let backing_cleanup = match options.backing_cleanup {
        BackingCleanupPolicy::DeleteManaged => match tokio::fs::try_exists(&client_config).await {
            Ok(true) => BackingCleanupPolicy::DeleteManaged,
            // Client state is deleted after backing cleanup, so a retry of an
            // interrupted removal has no redirections left to plan from.
            Ok(false) => {
                warning_handler(RemoveCheckoutWarning::BackingCleanupSkipped {
                    path: path.clone(),
                });
                BackingCleanupPolicy::Preserve
            }
            Err(error) => {
                warning_handler(RemoveCheckoutWarning::BackingCleanupFailed {
                    path: path.clone(),
                    error: format!("failed to check {}: {error}", client_config.display()),
                });
                BackingCleanupPolicy::Preserve
            }
        },
        BackingCleanupPolicy::Preserve => BackingCleanupPolicy::Preserve,
    };

    match redirection_cleanup_policy(checkout_is_active, backing_cleanup) {
        Some(BackingCleanupPolicy::DeleteManaged) => {
            // `.eden-redirections` is checkout content, so only client config redirections
            // can be planned while the checkout is unmounted.
            if !checkout_is_active {
                warning_handler(RemoveCheckoutWarning::RepositoryRedirectionCleanupSkipped {
                    path: path.clone(),
                });
            }
            clean_up_managed_backing(
                instance,
                &path,
                checkout_is_active,
                options.auxiliary_process_timeout,
                &mut event_handler,
            )
            .await;
        }
        Some(BackingCleanupPolicy::Preserve) => {
            unmount_redirections_with_timeout(
                &path,
                options.auxiliary_process_timeout,
                unmount_redirections(instance, &path),
                &mut warning_handler,
            )
            .await;
        }
        None => {}
    }

    if checkout_is_active {
        remove_active_checkout(instance, &path, options).await
    } else {
        remove_inactive_checkout(instance, &path, options).await
    }
}

fn redirection_cleanup_policy(
    checkout_is_active: bool,
    backing_cleanup: BackingCleanupPolicy,
) -> Option<BackingCleanupPolicy> {
    // A redirection mount can outlive the checkout root, so managed cleanup
    // always tries to unmount redirections before deleting their backing.
    match (checkout_is_active, backing_cleanup) {
        (_, BackingCleanupPolicy::DeleteManaged) => Some(BackingCleanupPolicy::DeleteManaged),
        (true, BackingCleanupPolicy::Preserve) => Some(BackingCleanupPolicy::Preserve),
        (false, BackingCleanupPolicy::Preserve) => None,
    }
}

struct RedirectionCleanup {
    redirection: Redirection,
    actions: Vec<RedirectionBackingCleanupAction>,
}

struct ManagedBackingCleanupPlan {
    checkout: EdenFsCheckout,
    redirections: Vec<RedirectionCleanup>,
    /// Redirections whose backing could not be planned. They are still unmounted.
    failures: Vec<String>,
}

async fn plan_managed_backing_cleanup(
    instance: &EdenFsInstance,
    path: &Path,
    timeout: Duration,
) -> Result<ManagedBackingCleanupPlan> {
    let instance = instance.clone();
    let path = path.to_path_buf();
    let path_for_error = path.clone();
    run_blocking_operation_with_timeout("eden-backing-cleanup-plan", timeout, move || {
        let checkout = find_checkout(&instance, &path)
            .with_context(|| format!("failed to find checkout for {}", path.display()))?;
        let mut redirections = Vec::new();
        let mut failures = Vec::new();
        let mut planner = RedirectionBackingCleanupPlanner::new(&path);
        for (repo_path, redirection) in get_effective_redirections(&instance, &checkout)
            .with_context(|| format!("failed to get redirections for {}", path.display()))?
        {
            // Plan each redirection on its own so one that fails validation doesn't
            // keep the others' backing.
            let label = repo_path.display().to_string();
            let planned = BTreeMap::from([(repo_path, redirection)]);
            let actions = planner.plan(&planned).unwrap_or_else(|error| {
                failures.push(format!("{label}: {error:#}"));
                Vec::new()
            });
            let redirection = planned
                .into_values()
                .next()
                .expect("the single planned redirection should still be present");
            redirections.push(RedirectionCleanup {
                redirection,
                actions,
            });
        }
        Ok(ManagedBackingCleanupPlan {
            checkout,
            redirections,
            failures,
        })
    })
    .await
    .with_context(|| {
        format!(
            "failed to plan managed redirection backing cleanup for {}",
            path_for_error.display()
        )
    })
}

async fn run_blocking_operation_with_timeout<T, Operation>(
    thread_name: &'static str,
    timeout: Duration,
    operation: Operation,
) -> Result<T>
where
    T: Send + 'static,
    Operation: FnOnce() -> Result<T> + Send + 'static,
{
    // A timed-out filesystem call can stay blocked; Tokio runtime shutdown must not wait for it.
    let (result_sender, result_receiver) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name(thread_name.to_owned())
        .spawn(move || {
            let _ = result_sender.send(operation());
        })
        .with_context(|| format!("failed to start {thread_name} task"))?;

    match tokio::time::timeout(timeout, result_receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => Err(anyhow!("{thread_name} task failed")),
        Err(_) => Err(anyhow!(
            "{thread_name} task timed out after {} seconds; it may continue until this process exits",
            timeout.as_secs_f64()
        )),
    }
}

fn get_test_delay() -> Option<Duration> {
    std::env::var("TEST_ONLY_AUX_PROCESSES_STOP_DELAY_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn unmount_redirections(instance: &EdenFsInstance, path: &Path) -> Result<()> {
    if let Some(delay) = get_test_delay() {
        debug!("Injecting test delay of {} seconds", delay.as_secs());
        tokio::time::sleep(delay).await;
    }

    let checkout = find_checkout(instance, path)
        .with_context(|| format!("failed to find checkout for {}", path.display()))?;
    let redirections = get_effective_redirections(instance, &checkout)
        .with_context(|| format!("failed to get redirections for {}", path.display()))?;

    for redirection in redirections.values() {
        redirection
            .remove_existing(instance, &checkout, false, false, "eden rm")
            .await
            .with_context(|| {
                format!(
                    "failed to unmount redirection {}",
                    redirection.repo_path().display()
                )
            })?;
    }
    Ok(())
}

async fn unmount_redirections_with_timeout<UnmountFuture, WarningHandler>(
    path: &Path,
    timeout: Duration,
    unmount: UnmountFuture,
    warning_handler: &mut WarningHandler,
) where
    UnmountFuture: Future<Output = Result<()>>,
    WarningHandler: FnMut(RemoveCheckoutWarning),
{
    match tokio::time::timeout(timeout, unmount).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warning_handler(RemoveCheckoutWarning::RedirectionUnmountFailed {
            path: path.to_path_buf(),
            error: format!("{error:#}"),
        }),
        Err(_) => warning_handler(RemoveCheckoutWarning::RedirectionUnmountTimedOut {
            path: path.to_path_buf(),
            timeout,
        }),
    }
}

async fn remove_active_checkout(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
) -> Result<()> {
    edenfs_core::unmount::unmount_for_removal(instance, path, options.no_force)
        .await
        .with_context(|| format!("failed to unmount mount point at {}", path.display()))?;
    remove_inactive_checkout(instance, path, options).await
}

/// How much of a redirection's backing can be deleted after trying to unmount it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BackingDeletionScope {
    /// The redirection is unmounted, so its backing targets can be deleted.
    Target,
    /// The redirection may still be mounted, so its backing targets are kept but emptied.
    Contents,
}

/// Unmount every redirection and delete its managed backing. This is best effort:
/// failures are reported as warnings so they never block checkout removal.
async fn clean_up_managed_backing<EventHandler>(
    instance: &EdenFsInstance,
    path: &Path,
    checkout_is_active: bool,
    timeout: Duration,
    event_handler: &mut EventHandler,
) where
    EventHandler: FnMut(RemoveCheckoutEvent),
{
    let ManagedBackingCleanupPlan {
        checkout,
        redirections,
        failures,
    } = match plan_managed_backing_cleanup(instance, path, timeout).await {
        Ok(plan) => plan,
        Err(error) => {
            event_handler(RemoveCheckoutEvent::Warning(
                RemoveCheckoutWarning::BackingCleanupFailed {
                    path: path.to_path_buf(),
                    error: format!("{error:#}"),
                },
            ));
            return;
        }
    };
    if !failures.is_empty() {
        event_handler(RemoveCheckoutEvent::Warning(
            RemoveCheckoutWarning::BackingCleanupFailed {
                path: path.to_path_buf(),
                error: format!(
                    "failed to plan backing cleanup for {} redirection(s):\n{}",
                    failures.len(),
                    failures.join("\n")
                ),
            },
        ));
    }

    #[cfg(target_os = "linux")]
    let scopes = unmount_managed_redirections(
        instance,
        path,
        &checkout,
        &redirections,
        checkout_is_active,
        timeout,
        &mut |warning| event_handler(RemoveCheckoutEvent::Warning(warning)),
    )
    .await
    .unwrap_or_else(|error| {
        event_handler(RemoveCheckoutEvent::Warning(
            RemoveCheckoutWarning::RedirectionUnmountFailed {
                path: path.to_path_buf(),
                error: format!("{error:#}"),
            },
        ));
        vec![BackingDeletionScope::Contents; redirections.len()]
    });

    #[cfg(not(target_os = "linux"))]
    let checkout = &checkout;
    #[cfg(not(target_os = "linux"))]
    let scopes = unmount_each_redirection(
        path,
        timeout,
        Instant::now(),
        &redirections,
        |cleanup| async move {
            let redirection = &cleanup.redirection;
            let unmounted = redirection
                .unmount_for_backing_cleanup(instance, checkout)
                .await
                .with_context(|| {
                    format!(
                        "failed to unmount redirection {}",
                        redirection.repo_path().display()
                    )
                });
            // Bind unmounts go through the daemon, which only serves mounted checkouts.
            if checkout_is_active {
                unmounted
            } else {
                unmounted.with_context(|| {
                    format!(
                        "{} is not mounted, so EdenFS cannot unmount its redirections",
                        path.display()
                    )
                })
            }
        },
        &mut |warning| event_handler(RemoveCheckoutEvent::Warning(warning)),
    )
    .await;

    let deletions = redirections
        .into_iter()
        .zip(scopes)
        .flat_map(|(cleanup, scope)| {
            cleanup
                .actions
                .into_iter()
                .map(move |action| (action, scope))
        })
        .collect();
    if let Err(error) = execute_backing_cleanup_actions(deletions, event_handler).await {
        event_handler(RemoveCheckoutEvent::Warning(
            RemoveCheckoutWarning::BackingCleanupFailed {
                path: path.to_path_buf(),
                error: format!("{error:#}"),
            },
        ));
    }
}

#[cfg(target_os = "linux")]
async fn unmount_managed_redirections(
    instance: &EdenFsInstance,
    path: &Path,
    checkout: &EdenFsCheckout,
    redirections: &[RedirectionCleanup],
    checkout_is_active: bool,
    timeout: Duration,
    warning_handler: &mut impl FnMut(RemoveCheckoutWarning),
) -> Result<Vec<BackingDeletionScope>> {
    if redirections.is_empty() {
        return Ok(Vec::new());
    }
    let start = Instant::now();
    let checkout_path = checkout.path();
    let paths: Vec<_> = redirections
        .iter()
        .map(|cleanup| checkout_path.join(cleanup.redirection.repo_path()))
        .collect();
    let before_paths = paths.clone();
    let mounted =
        run_blocking_operation_with_timeout("eden-redirection-mounts", timeout, move || {
            Ok(redirection_mount_status(&before_paths)?)
        })
        .await
        .context("failed to inspect redirection mounts")?;
    let candidates: Vec<_> = redirections.iter().zip(mounted).collect();
    let scopes = unmount_each_redirection(
        path,
        timeout,
        start,
        &candidates,
        |(cleanup, mounted)| async move {
            if *mounted {
                let unmounted = cleanup
                    .redirection
                    .detach_for_backing_cleanup(instance, checkout)
                    .await
                    .with_context(|| {
                        format!(
                            "failed to unmount redirection {}",
                            cleanup.redirection.repo_path().display()
                        )
                    });
                if checkout_is_active {
                    unmounted?;
                } else {
                    unmounted.with_context(|| {
                        format!(
                            "{} is not mounted, so EdenFS cannot unmount its redirections",
                            path.display()
                        )
                    })?;
                }
            }
            Ok(())
        },
        warning_handler,
    )
    .await;
    if !scopes.contains(&BackingDeletionScope::Target) {
        return Ok(scopes);
    }
    let remaining = timeout.saturating_sub(start.elapsed());
    if remaining.is_zero() {
        return Err(anyhow!("redirection unmount verification timed out"));
    }

    // All mount changes finish before this fresh observation. Path cleanup below
    // only unlinks symlinks or removes empty directories, so it cannot detach mounts.
    let verified =
        run_blocking_operation_with_timeout("eden-redirection-verify", remaining, move || {
            let mounted = redirection_mount_status(&paths)?;
            Ok(paths
                .iter()
                .zip(mounted)
                .zip(scopes)
                .map(|((repo_path, mounted), scope)| {
                    if scope == BackingDeletionScope::Contents {
                        return Ok(scope);
                    }
                    let unmounted = if mounted {
                        Err(anyhow!(
                            "redirection {} is still mounted after unmount",
                            repo_path.display()
                        ))
                    } else {
                        finish_backing_cleanup_unmount(repo_path).map_err(anyhow::Error::from)
                    };
                    unmounted.with_context(|| {
                        format!(
                            "failed to unmount redirection {}",
                            repo_path
                                .strip_prefix(&checkout_path)
                                .unwrap_or(repo_path)
                                .display()
                        )
                    })?;
                    Ok(BackingDeletionScope::Target)
                })
                .collect::<Vec<Result<_>>>())
        })
        .await
        .context("failed to verify redirection unmounts")?;
    Ok(verified
        .into_iter()
        .map(|result| match result {
            Ok(scope) => scope,
            Err(error) => {
                warning_handler(RemoveCheckoutWarning::RedirectionUnmountFailed {
                    path: path.to_path_buf(),
                    error: format!("{error:#}"),
                });
                BackingDeletionScope::Contents
            }
        })
        .collect())
}

/// Unmount each redirection within one shared deadline and return how much of each one's
/// backing can then be deleted. Failures are reported as warnings.
async fn unmount_each_redirection<'a, T, Unmount, UnmountFuture, WarningHandler>(
    path: &Path,
    timeout: Duration,
    start: Instant,
    redirections: &'a [T],
    unmount: Unmount,
    warning_handler: &mut WarningHandler,
) -> Vec<BackingDeletionScope>
where
    Unmount: Fn(&'a T) -> UnmountFuture,
    UnmountFuture: Future<Output = Result<()>>,
    WarningHandler: FnMut(RemoveCheckoutWarning),
{
    let mut timed_out = false;
    let mut scopes = Vec::with_capacity(redirections.len());
    for redirection in redirections {
        let remaining = timeout.saturating_sub(start.elapsed());
        let unmounted = if remaining.is_zero() {
            None
        } else {
            tokio::time::timeout(remaining, unmount(redirection))
                .await
                .ok()
        };
        scopes.push(match unmounted {
            Some(Ok(())) => BackingDeletionScope::Target,
            Some(Err(error)) => {
                warning_handler(RemoveCheckoutWarning::RedirectionUnmountFailed {
                    path: path.to_path_buf(),
                    error: format!("{error:#}"),
                });
                BackingDeletionScope::Contents
            }
            None => {
                timed_out = true;
                BackingDeletionScope::Contents
            }
        });
    }
    if timed_out {
        warning_handler(RemoveCheckoutWarning::RedirectionUnmountTimedOut {
            path: path.to_path_buf(),
            timeout,
        });
    }
    scopes
}

async fn execute_backing_cleanup_actions(
    deletions: Vec<(RedirectionBackingCleanupAction, BackingDeletionScope)>,
    event_handler: &mut impl FnMut(RemoveCheckoutEvent),
) -> Result<()> {
    execute_backing_cleanup_actions_with(
        deletions,
        |(action, _)| action.target().to_path_buf(),
        |(action, scope)| {
            match scope {
                BackingDeletionScope::Target => action.execute(),
                BackingDeletionScope::Contents => action.execute_contents(),
            }
            .map_err(Into::into)
        },
        event_handler,
    )
    .await
}

async fn execute_backing_cleanup_actions_with<Action, Target, Execute>(
    actions: Vec<Action>,
    target: Target,
    execute: Execute,
    event_handler: &mut impl FnMut(RemoveCheckoutEvent),
) -> Result<()>
where
    Action: Send + 'static,
    Target: Fn(&Action) -> PathBuf,
    Execute: Fn(Action) -> Result<()> + Clone + Send + Sync + 'static,
{
    if !actions.is_empty() {
        event_handler(RemoveCheckoutEvent::DeletingBackingStorage);
    }
    let mut failures = Vec::new();
    for action in actions {
        let action_target = target(&action);
        let execute = execute.clone();
        let result = tokio::task::spawn_blocking(move || execute(action))
            .await
            .context("backing cleanup worker failed")
            .and_then(|result| result)
            .with_context(|| {
                format!(
                    "failed to delete redirection backing target {}",
                    action_target.display()
                )
            });
        if let Err(error) = result {
            failures.push(format!("{}: {error:#}", action_target.display()));
        }
    }

    if !failures.is_empty() {
        return Err(anyhow!(
            "failed to delete {} redirection backing target(s):\n{}",
            failures.len(),
            failures.join("\n")
        ));
    }

    Ok(())
}

async fn remove_inactive_checkout(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
) -> Result<()> {
    remove_client_config_dir(instance, path)?;
    instance
        .remove_path_from_directory_map(path)
        .with_context(|| format!("failed to remove {} from config json file", path.display()))?;

    if !options.preserve_mount_point {
        clean_mount_point(path)?;
    }
    validate_removal_completion(instance, path, options.preserve_mount_point).await
}

fn remove_client_config_dir(instance: &EdenFsInstance, path: &Path) -> Result<()> {
    let client_dir = instance.client_dir_for_mount_point(path)?;
    match forcefully_remove_dir_all(&client_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow!(
            "failed to remove client config directory for {}: {error}",
            path.display()
        )),
    }
}

/// Remove a directory using the same permission and retry handling as checkout removal.
pub fn clean_mount_point(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o755))
        .with_context(|| format!("failed to set permission 755 for path {}", path.display()))?;

    forcefully_remove_dir_all(path)
        .with_context(|| format!("failed to remove mount point {}", path.display()))
}

async fn validate_removal_completion(
    instance: &EdenFsInstance,
    path: &Path,
    preserve_mount_point: bool,
) -> Result<()> {
    if preserve_mount_point {
        return Ok(());
    }

    if path_in_eden_config(instance, path).await? {
        return Err(anyhow!("repo {} is still mounted", path.display()));
    }

    fail_point!("remove:validate", |_| {
        Err(anyhow!("failpoint: expected failure"))
    });

    match path.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(anyhow!(
            "directory left by repo {} is not removed",
            path.display()
        )),
        Err(error) => Err(anyhow!(
            "failed to check the status of path {}: {error}",
            path.display()
        )),
    }
}

/// Return whether a path is still registered in the EdenFS directory map.
pub async fn path_in_eden_config(instance: &EdenFsInstance, path: &Path) -> Result<bool> {
    let mut mounts = get_mounts(instance)
        .await
        .context("failed to call eden list")?;
    let entry_key = dunce::simplified(path);
    mounts.retain(|mount_path, _| dunce::simplified(mount_path) == entry_key);
    Ok(!mounts.is_empty())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::sync::Arc;

    use edenfs_core::daemon::DisconnectedDaemon;
    use tempfile::tempdir;

    use super::*;

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn batch_unmount_cleans_paths_without_following_symlinks() {
        use edenfs_core::redirect::RedirectionState;
        use edenfs_core::redirect::RedirectionType;

        let temp = tempdir().unwrap();
        let config_dir = temp.path().join("eden");
        let client_dir = config_dir.join("clients/test");
        let path = temp.path().join("checkout");
        fs::create_dir_all(&client_dir).unwrap();
        fs::create_dir(&path).unwrap();
        fs::write(
            config_dir.join("config.json"),
            serde_json::to_vec(&BTreeMap::from([(&path, "test")])).unwrap(),
        )
        .unwrap();
        fs::write(
            client_dir.join("config.toml"),
            "[repository]\npath = '/tmp'\ntype = 'hg'\n[redirections]\n",
        )
        .unwrap();
        let instance = EdenFsInstance::with_daemon(
            config_dir.clone(),
            config_dir,
            None,
            Arc::new(DisconnectedDaemon),
        );
        let checkout = find_checkout(&instance, &path).unwrap();
        let target = temp.path().join("backing");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("data"), "keep").unwrap();
        let redirections: Vec<_> = (0..64)
            .map(|index| {
                let repo_path = PathBuf::from(format!("redir-{index}"));
                let full_path = path.join(&repo_path);
                match index % 4 {
                    0 => fs::create_dir(&full_path).unwrap(),
                    1 => std::os::unix::fs::symlink(&target, &full_path).unwrap(),
                    2 => {
                        fs::create_dir(&full_path).unwrap();
                        fs::write(full_path.join("local"), "keep").unwrap();
                    }
                    _ => fs::write(&full_path, "keep").unwrap(),
                }
                RedirectionCleanup {
                    redirection: Redirection {
                        repo_path,
                        redir_type: RedirectionType::Bind,
                        source: String::new(),
                        state: RedirectionState::NotMounted,
                        target: None,
                    },
                    actions: Vec::new(),
                }
            })
            .collect();
        let mut warnings = Vec::new();
        let scopes = unmount_managed_redirections(
            &instance,
            &path,
            &checkout,
            &redirections,
            false,
            Duration::from_secs(10),
            &mut |warning| warnings.push(warning),
        )
        .await
        .unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(scopes, vec![BackingDeletionScope::Target; 64]);
        for index in 0..64 {
            let full_path = path.join(format!("redir-{index}"));
            match index % 4 {
                0 | 1 => assert!(!fs::symlink_metadata(full_path).is_ok()),
                2 => assert_eq!(fs::read_to_string(full_path.join("local")).unwrap(), "keep"),
                _ => assert_eq!(fs::read_to_string(full_path).unwrap(), "keep"),
            }
        }
        assert_eq!(fs::read_to_string(target.join("data")).unwrap(), "keep");
    }

    #[tokio::test]
    async fn unmount_batch_keeps_successes_before_shared_deadline() {
        let mut warnings = Vec::new();
        let attempts = std::cell::Cell::new(0);
        let scopes = unmount_each_redirection(
            Path::new("/checkout"),
            Duration::from_millis(20),
            Instant::now(),
            &[0, 1, 2],
            |index| {
                attempts.set(attempts.get() + 1);
                async move {
                    if *index == 1 {
                        std::future::pending::<()>().await;
                    }
                    Ok(())
                }
            },
            &mut |warning| warnings.push(warning),
        )
        .await;
        assert_eq!(attempts.get(), 2);
        assert_eq!(
            scopes,
            [
                BackingDeletionScope::Target,
                BackingDeletionScope::Contents,
                BackingDeletionScope::Contents
            ]
        );
        assert!(matches!(
            warnings.as_slice(),
            [RemoveCheckoutWarning::RedirectionUnmountTimedOut { .. }]
        ));
    }

    #[tokio::test]
    async fn unmount_failure_limits_backing_deletion_to_contents() {
        let mut warnings = Vec::new();

        let scopes = unmount_each_redirection(
            Path::new("/checkout"),
            Duration::from_secs(10),
            Instant::now(),
            &["buck-out", "stuck", "other"],
            |repo_path| async move {
                if *repo_path == "stuck" {
                    Err(anyhow!("device busy"))
                } else {
                    Ok(())
                }
            },
            &mut |warning| warnings.push(warning),
        )
        .await;

        assert_eq!(
            scopes,
            [
                BackingDeletionScope::Target,
                BackingDeletionScope::Contents,
                BackingDeletionScope::Target
            ],
            "only the redirection that stayed mounted keeps its backing target"
        );
        assert_eq!(
            warnings,
            vec![RemoveCheckoutWarning::RedirectionUnmountFailed {
                path: PathBuf::from("/checkout"),
                error: "device busy".to_owned(),
            }]
        );
    }

    #[tokio::test]
    async fn reports_redirection_unmount_failure() {
        let mut warnings = Vec::new();

        unmount_redirections_with_timeout(
            Path::new("/checkout"),
            Duration::from_secs(10),
            async { Err(anyhow!("device busy").context("failed to unmount buck-out")) },
            &mut |warning| warnings.push(warning),
        )
        .await;

        assert_eq!(warnings.len(), 1, "the caller must receive the failure");
        assert!(
            warnings[0]
                .to_string()
                .contains("failed to unmount buck-out: device busy"),
            "the warning must retain the underlying cause"
        );
    }

    #[tokio::test]
    async fn delete_managed_unregisters_checkout_with_missing_client_config() {
        let temp_dir = tempdir().expect("temporary directory");
        let config_dir = temp_dir.path().join("eden");
        let checkout = temp_dir.path().join("checkout");
        fs::create_dir_all(&config_dir).expect("config directory");
        fs::create_dir_all(&checkout).expect("checkout directory");
        let checkout = checkout.canonicalize().expect("checkout path");
        fs::write(
            config_dir.join("config.json"),
            serde_json::to_vec(&BTreeMap::from([(&checkout, "missing-client")]))
                .expect("directory map"),
        )
        .expect("write directory map");
        let instance = EdenFsInstance::with_daemon(
            config_dir.clone(),
            config_dir,
            None,
            Arc::new(DisconnectedDaemon),
        );
        let mut warnings = Vec::new();

        remove_checkout_with_warning_handler(
            &instance,
            &checkout,
            RemoveCheckoutOptions {
                backing_cleanup: BackingCleanupPolicy::DeleteManaged,
                ..RemoveCheckoutOptions::default()
            },
            |warning| warnings.push(warning),
        )
        .await
        .expect("missing client state must not block unregistering the checkout");

        assert_eq!(
            warnings,
            vec![RemoveCheckoutWarning::BackingCleanupSkipped {
                path: checkout.clone()
            }]
        );
        assert!(!checkout.exists());
        assert!(
            instance
                .get_configured_mounts_map()
                .expect("directory map")
                .is_empty()
        );
    }
}
