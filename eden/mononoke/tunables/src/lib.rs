/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;
use std::thread_local;

use anyhow::Result;
use arc_swap::ArcSwap;
use arc_swap::ArcSwapOption;
use cached_config::ConfigHandle;
use futures::future::poll_fn;
use futures::Future;
use futures::FutureExt;
use slog::debug;
use slog::error;
use slog::warn;
use slog::Logger;
use stats::prelude::*;
use tokio::runtime::Handle;
use tunables_derive::Tunables;
use tunables_structs::Tunables as TunablesStruct;

define_stats! {
    prefix = "mononoke.tunables";
    refresh_failure_count: timeseries(Average, Sum, Count),
}

static TUNABLES: OnceLock<MononokeTunables> = OnceLock::new();
static TUNABLES_WORKER_STATE: OnceLock<TunablesWorkerState> = OnceLock::new();

thread_local! {
    static TUNABLES_OVERRIDE: RefCell<Option<Arc<MononokeTunables>>> = RefCell::new(None);
}

pub enum TunablesReference {
    Override(Arc<MononokeTunables>),
    Static(&'static MononokeTunables),
}

impl Deref for TunablesReference {
    type Target = MononokeTunables;

    fn deref(&self) -> &MononokeTunables {
        match self {
            Self::Override(r) => r.as_ref(),
            Self::Static(r) => r,
        }
    }
}

pub fn tunables() -> TunablesReference {
    TUNABLES_OVERRIDE.with(|tunables_override| match *tunables_override.borrow() {
        Some(ref arc) => TunablesReference::Override(arc.clone()),
        None => TunablesReference::Static(TUNABLES.get_or_init(MononokeTunables::default)),
    })
}

// These types exist to simplify code generation in tunables-derive
pub type TunableBool = ArcSwapOption<bool>;
pub type TunableI64 = ArcSwapOption<i64>;
pub type TunableU64 = ArcSwapOption<u64>;
pub type TunableString = ArcSwapOption<String>;
pub type TunableVecOfStrings = ArcSwapOption<Vec<String>>;

pub type TunableBoolByRepo = ArcSwap<HashMap<String, bool>>;
pub type TunableI64ByRepo = ArcSwap<HashMap<String, i64>>;
pub type TunableU64ByRepo = ArcSwap<HashMap<String, u64>>;
pub type TunableStringByRepo = ArcSwap<HashMap<String, String>>;
pub type TunableVecOfStringsByRepo = ArcSwap<HashMap<String, Vec<String>>>;

#[derive(Tunables, Default, Debug)]
pub struct MononokeTunables {
    filenodes_disabled: TunableBool,
    warm_bookmark_cache_poll_interval_ms: TunableI64,
    disable_repo_client_warm_bookmarks_cache: TunableBool,
    xrepo_sync_disable_all_syncs: TunableBool,
    xrepo_disable_commit_sync_lease: TunableBool,
    allow_change_xrepo_mapping_extra: TunableBool,
    // What timeout to use when doing filenode lookup.
    // Usually filenode lookup is used while generating hg changesets
    filenode_lookup_timeout_ms: TunableI64,
    // Sampling ratio percentage for warm boomark cache.
    warm_bookmark_cache_logging_sampling_pct: TunableI64,
    // Control whether the BYPASS_READONLY pushvar is restricted by an ACL
    enforce_bypass_readonly_acl: TunableBool,
    // Boolean to batch requests sent to Land Service
    batching_to_land_service: TunableBool,
    // Which region writes should be done to, in order to minimise latency.
    // This should align with underlying storage (SQL/Manifold) write regions.
    // Notice writes still work from any region, and this field is not necessarily
    // enforced.
    preferred_write_region: TunableString,
    // The replication_status call is problematic for SQL so we're experimenting
    // with removing it, but this tunable can be used as a quick killswitch to
    // enable them again.
    sql_lag_monitoring_blocklist: TunableVecOfStrings,
    // If set, the hook won't be created at all
    disable_check_write_permissions_hook: TunableBool,
    // If set, the check result will be discarded for user identities
    log_only_for_users_in_cwp_hook: TunableBool,
    // If set, the check result will be discarded for service identities
    log_only_for_services_in_cwp_hook: TunableBool,
    // If set, the wireproto implementation will only log the repo write ACL
    // check result.
    log_only_wireproto_write_acl: TunableBool,
    // If set the `draft` ACL action will be checked and logged on draft access
    // Unless `enforce_draft_acl` is set `read` action will still be used for
    // granting access.
    log_draft_acl_failures: TunableBool,
    // If set the `draft` ACL action will be used for `draft` access.
    enforce_draft_acl: TunableBool,
    // Force local pushrebase instead of talking to SCS or Land Service
    force_local_pushrebase: TunableBool,
    // Enable usage of basename_suffix_skeleton_manifest in commit_find_files
    disable_basename_suffix_skeleton_manifest: TunableBool,
    // Enable using BSSM for suffix queries. Might be inneficient for broad suffixes (like .php)
    enable_bssm_suffix_query: TunableBool,
    // Enable using optimized BSSM derivation.
    enable_bssm_optimized_derivation: TunableBool,
    // List of targets in AOSP megarepo to apply squashing config overrides
    megarepo_squashing_config_override_targets: TunableVecOfStringsByRepo,
    // Override squashing limit for listed targets
    megarepo_override_squashing_limit: TunableI64ByRepo,
    // Override author check during squashing
    megarepo_override_author_check: TunableBoolByRepo,
    // Skip backsyncing for empty commits (except mapping changes via extras and merges)
    cross_repo_skip_backsyncing_ordinary_empty_commits: TunableBoolByRepo,
}

fn log_tunables(tunables: &TunablesStruct) -> String {
    serde_json::to_string(tunables)
        .unwrap_or_else(|e| format!("failed to serialize tunables: {}", e))
}

pub fn init_tunables_worker(
    logger: Logger,
    config_handle: ConfigHandle<TunablesStruct>,
    runtime_handle: Handle,
) -> Result<()> {
    init_tunables(&logger, &config_handle)?;
    if TUNABLES_WORKER_STATE
        .set(TunablesWorkerState {
            config_handle,
            logger,
        })
        .is_err()
    {
        panic!("Two or more tunables update threads exist at the same time");
    }
    runtime_handle.spawn(wait_and_update());

    Ok(())
}

pub fn init_tunables(logger: &Logger, config_handle: &ConfigHandle<TunablesStruct>) -> Result<()> {
    let tunables = config_handle.get();
    debug!(logger, "Initializing tunables: {}", log_tunables(&tunables));
    update_tunables(tunables)
}

/// Tunables are updated when the underlying config source notifies of a change.
/// Call this to force update them to the latest value provided by the config source.
/// Meant to be used in tests.
/// NOTE: if tunables are fetched from Configerator, you need to force update it as well.
pub fn force_update_tunables() {
    let state = TUNABLES_WORKER_STATE
        .get()
        .expect("Tunables worker state uninitialised");
    wait_and_update_iteration(state.config_handle.get(), &state.logger);
}

struct TunablesWorkerState {
    config_handle: ConfigHandle<TunablesStruct>,
    logger: Logger,
}

async fn wait_and_update() {
    let state = TUNABLES_WORKER_STATE
        .get()
        .expect("Tunables worker state uninitialised");
    let mut config_watcher = state
        .config_handle
        .watcher()
        .expect("Tunable backed by static config source");
    loop {
        match config_watcher.wait_for_next().await {
            Ok(new_tunables) => wait_and_update_iteration(new_tunables, &state.logger),
            Err(e) => {
                error!(
                    state.logger,
                    "Error in fetching latest config for tunable: {}.\n Exiting tunable updater", e
                );
                // Set the refresh failure count counter so that the oncall can be alerted
                // based on this metric
                STATS::refresh_failure_count.add_value(1);
                return;
            }
        }
    }
}

fn wait_and_update_iteration(new_tunables: Arc<TunablesStruct>, logger: &Logger) {
    debug!(
        logger,
        "Updating tunables to new: {}",
        log_tunables(&new_tunables),
    );
    if let Err(e) = update_tunables(new_tunables) {
        warn!(logger, "Failed to refresh tunables: {}", e);
        // Set the refresh failure count counter so that the oncall can be alerted
        // based on this metric
        STATS::refresh_failure_count.add_value(1);
    } else {
        // Add a value of 0 so that the counter won't get dead even if there
        // are no errors
        STATS::refresh_failure_count.add_value(0);
    }
}

fn update_tunables(new_tunables: Arc<TunablesStruct>) -> Result<()> {
    let tunables = tunables();
    tunables.update_bools(&new_tunables.killswitches);
    tunables.update_ints(&new_tunables.ints);
    tunables.update_strings(&new_tunables.strings);
    tunables.update_vec_of_strings(&new_tunables.vec_of_strings);

    if let Some(killswitches_by_repo) = &new_tunables.killswitches_by_repo {
        tunables.update_by_repo_bools(killswitches_by_repo);
    }

    if let Some(ints_by_repo) = &new_tunables.ints_by_repo {
        tunables.update_by_repo_ints(ints_by_repo);
    }

    if let Some(vec_of_strings_by_repo) = &new_tunables.vec_of_strings_by_repo {
        tunables.update_by_repo_vec_of_strings(vec_of_strings_by_repo);
    }
    Ok(())
}

/// A helper function to override tunables during a closure's execution.
/// This is useful for unit tests.
pub fn with_tunables<T>(new_tunables: MononokeTunables, f: impl FnOnce() -> T) -> T {
    TUNABLES_OVERRIDE.with(|t| *t.borrow_mut() = Some(Arc::new(new_tunables)));

    let res = f();

    TUNABLES_OVERRIDE.with(|tunables| *tunables.borrow_mut() = None);

    res
}

pub fn with_tunables_async<Out, Fut: Future<Output = Out> + Unpin>(
    new_tunables: MononokeTunables,
    fut: Fut,
) -> impl Future<Output = Out> {
    with_tunables_async_arc(Arc::new(new_tunables), fut)
}

pub fn with_tunables_async_arc<Out, Fut: Future<Output = Out> + Unpin>(
    new_tunables: Arc<MononokeTunables>,
    mut fut: Fut,
) -> impl Future<Output = Out> {
    poll_fn(move |cx| {
        TUNABLES_OVERRIDE.with(|t| *t.borrow_mut() = Some(new_tunables.clone()));

        let res = fut.poll_unpin(cx);

        TUNABLES_OVERRIDE.with(|tunables| *tunables.borrow_mut() = None);

        res
    })
}

pub fn override_tunables(new_tunables: Option<Arc<MononokeTunables>>) {
    TUNABLES_OVERRIDE.with(|t| *t.borrow_mut() = new_tunables);
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use maplit::hashmap;

    use super::*;

    #[derive(Tunables, Default)]
    struct TestTunables {
        boolean: TunableBool,
        num: TunableI64,
        unum: TunableU64,
        string: TunableString,
        vecofstrings: TunableVecOfStrings,

        repobool: TunableBoolByRepo,
        repobool2: TunableBoolByRepo,

        repoint: TunableI64ByRepo,
        repoint2: TunableI64ByRepo,
        repouint: TunableU64ByRepo,

        repostr: TunableStringByRepo,
        repostr2: TunableStringByRepo,

        repovecofstrings: TunableVecOfStringsByRepo,
    }

    #[derive(Tunables, Default)]
    struct EmptyTunables {}

    fn s(a: &str) -> String {
        a.to_string()
    }

    #[test]
    fn test_override_tunables() {
        assert!(tunables().warm_bookmark_cache_poll_interval_ms().is_none());

        let res = with_tunables(
            MononokeTunables {
                warm_bookmark_cache_poll_interval_ms: ArcSwapOption::from(Some(Arc::new(2))),
                ..MononokeTunables::default()
            },
            || {
                tunables()
                    .warm_bookmark_cache_poll_interval_ms()
                    .unwrap_or_default()
            },
        );

        assert_eq!(res, 2);
        assert!(tunables().warm_bookmark_cache_poll_interval_ms().is_none());
    }

    #[test]
    fn test_empty_tunables() {
        let empty = EmptyTunables::default();

        empty.update_bools(&HashMap::new());
        empty.update_ints(&HashMap::new());
        empty.update_strings(&HashMap::new());
        empty.update_vec_of_strings(&HashMap::new());
    }

    #[test]
    fn test_update_bool() {
        let mut d = HashMap::new();
        d.insert(s("boolean"), true);

        let test = TestTunables::default();
        assert!(test.boolean().is_none());
        test.update_bools(&d);
        assert_eq!(test.boolean(), Some(true));
    }

    #[test]
    fn test_revert_update() {
        // booleans
        let mut d = hashmap! {};
        d.insert(s("boolean"), true);

        let test = TestTunables::default();
        assert!(test.boolean().is_none());
        test.update_bools(&d);
        assert_eq!(test.boolean(), Some(true));

        test.update_bools(&hashmap! {});
        assert!(test.boolean().is_none());

        // ints
        let test = TestTunables::default();
        test.update_ints(&hashmap! { s("num") => 1, s("unum") => 2});
        assert_eq!(test.num(), Some(1i64));
        assert_eq!(test.unum(), Some(2u64));

        test.update_ints(&hashmap! {});
        assert_eq!(test.num(), None);
        assert_eq!(test.unum(), None);

        // strings
        let test = TestTunables::default();
        test.update_strings(&hashmap! { s("string") => s("string")});
        assert_eq!(test.string(), Some(Arc::new(s("string"))));

        test.update_strings(&hashmap! {});
        assert!(test.string().is_none());

        // by repo bools
        assert_eq!(test.by_repo_repobool("repo"), None);
        assert_eq!(test.by_repo_repobool("repo2"), None);

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => true,
            },
            s("repo2") => hashmap! {
                s("repobool") => false,
            }
        });
        assert_eq!(test.by_repo_repobool("repo"), Some(true));
        assert_eq!(test.by_repo_repobool("repo2"), Some(false));

        test.update_by_repo_bools(&hashmap! {});
        assert_eq!(test.by_repo_repobool("repo"), None);
        assert_eq!(test.by_repo_repobool("repo2"), None);
    }

    #[test]
    fn test_update_int() {
        let mut d = HashMap::new();
        d.insert(s("num"), 10);
        // We store very large unsigned numbers as their bit-wise signed
        // equivalent, so a value like `u64::MAX` will be stored as -1.
        d.insert(s("unum"), -1);

        let test = TestTunables::default();
        assert!(test.num().is_none());
        assert!(test.unum().is_none());
        test.update_ints(&d);
        assert_eq!(test.num(), Some(10));
        assert_eq!(test.unum(), Some(u64::MAX));
    }

    #[test]
    fn test_missing_int() {
        let mut d = HashMap::new();
        d.insert(s("missing"), 10);

        let test = TestTunables::default();
        assert!(test.num().is_none());
        assert!(test.unum().is_none());
        test.update_ints(&d);
        assert!(test.num().is_none());
        assert!(test.unum().is_none());
    }

    #[test]
    fn update_string() {
        let mut d = HashMap::new();
        d.insert(s("string"), s("value"));

        let test = TestTunables::default();
        assert!(test.string().is_none());
        test.update_strings(&d);
        assert_eq!(test.string().unwrap().as_str(), "value");
    }

    #[test]
    fn update_vec_of_strings() {
        let mut d = HashMap::new();
        d.insert(s("vecofstrings"), vec![s("value"), s("value2")]);

        let test = TestTunables::default();
        assert!(&test.vecofstrings().is_none());
        test.update_vec_of_strings(&d);
        assert_eq!(
            &test.vecofstrings().unwrap().as_slice(),
            &[s("value"), s("value2")]
        );
    }

    #[test]
    fn update_by_repo_bool() {
        let test = TestTunables::default();

        assert_eq!(test.by_repo_repobool("repo"), None);
        assert_eq!(test.by_repo_repobool("repo2"), None);

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => true,
            },
            s("repo2") => hashmap! {
                s("repobool") => true,
            }
        });
        assert_eq!(test.by_repo_repobool("repo"), Some(true));
        assert_eq!(test.by_repo_repobool("repo2"), Some(true));

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => true,
            }
        });
        assert_eq!(test.by_repo_repobool("repo2"), None);

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => false,
            }
        });
        assert_eq!(test.by_repo_repobool("repo"), Some(false));

        test.update_by_repo_bools(&hashmap! {
            s(":default:") => hashmap! {
                s("repobool") => true,
            },
            s("repo") => hashmap! {
                s("repobool") => false,
            }
        });

        assert_eq!(test.by_repo_repobool("repo"), Some(false));
        assert_eq!(test.by_repo_repobool("repo2"), Some(true));

        test.update_by_repo_bools(&hashmap! {
            s(":override:") => hashmap! {
                s("repobool") => true,
            },
            s("repo") => hashmap! {
                s("repobool") => false,
            }
        });

        assert_eq!(test.by_repo_repobool("repo"), Some(true));
        assert_eq!(test.by_repo_repobool("repo2"), Some(true));
    }

    #[test]
    fn update_by_repo_two_bools() {
        let test = TestTunables::default();
        assert_eq!(test.by_repo_repobool("repo"), None);
        assert_eq!(test.by_repo_repobool2("repo"), None);

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => true,
                s("repobool2") => true,
            }
        });

        assert_eq!(test.by_repo_repobool("repo"), Some(true));
        assert_eq!(test.by_repo_repobool2("repo"), Some(true));

        test.update_by_repo_bools(&hashmap! {
            s("repo") => hashmap! {
                s("repobool") => true,
                s("repobool2") => false,
            }
        });

        assert_eq!(test.by_repo_repobool("repo"), Some(true));
        assert_eq!(test.by_repo_repobool2("repo"), Some(false));
    }

    #[test]
    fn update_by_repo_str() {
        let test = TestTunables::default();

        assert_eq!(test.by_repo_repostr("repo"), None);
        assert_eq!(test.by_repo_repostr("repo2"), None);

        test.update_by_repo_strings(&hashmap! {
            s("repo") => hashmap! {
                s("repostr") => s("hello"),
            },
            s("repo2") => hashmap! {
                s("repostr") => s("world"),
            },
        });
        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello")));
        assert_eq!(test.by_repo_repostr("repo2"), Some(s("world")));

        test.update_by_repo_strings(&hashmap! {
            s("repo") => hashmap! {
                s("repostr") => s("hello2"),
            },
        });
        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello2")));
        assert_eq!(test.by_repo_repostr("repo2"), None);

        test.update_by_repo_strings(&hashmap! {
            s(":default:") => hashmap! {
                s("repostr") => s("hello3")
            },
            s("repo") => hashmap! {
                s("repostr") => s("hello"),
            },
        });

        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello")));
        assert_eq!(test.by_repo_repostr("repo2"), Some(s("hello3")));

        test.update_by_repo_strings(&hashmap! {
            s(":override:") => hashmap! {
                s("repostr") => s("hello3")
            },
            s("repo") => hashmap! {
                s("repostr") => s("hello"),
            },
        });

        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello3")));
        assert_eq!(test.by_repo_repostr("repo2"), Some(s("hello3")));
    }

    #[test]
    fn update_by_repo_two_strs() {
        let test = TestTunables::default();
        assert_eq!(test.by_repo_repostr("repo"), None);
        assert_eq!(test.by_repo_repostr2("repo"), None);

        test.update_by_repo_strings(&hashmap! {
            s("repo") => hashmap! {
                s("repostr") => s("hello"),
                s("repostr2") => s("world"),
            }
        });

        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello")));
        assert_eq!(test.by_repo_repostr2("repo"), Some(s("world")));

        test.update_by_repo_strings(&hashmap! {
            s("repo") => hashmap! {
                s("repostr") => s("hello2"),
            }
        });

        assert_eq!(test.by_repo_repostr("repo"), Some(s("hello2")));
        assert_eq!(test.by_repo_repostr2("repo"), None);
    }

    #[test]
    fn update_by_repo_int() {
        let test = TestTunables::default();

        assert_eq!(test.by_repo_repoint("repo"), None);
        assert_eq!(test.by_repo_repoint("repo2"), None);

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repoint") => 1,
            },
            s("repo2") => hashmap! {
                s("repoint") => 2,
            },
        });
        assert_eq!(test.by_repo_repoint("repo"), Some(1));
        assert_eq!(test.by_repo_repoint("repo2"), Some(2));

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repoint") => 3,
            },
        });
        assert_eq!(test.by_repo_repoint("repo"), Some(3));
        assert_eq!(test.by_repo_repoint("repo2"), None);

        test.update_by_repo_ints(&hashmap! {
            s(":default:") => hashmap! {
                s("repoint") => 4
            },
            s("repo") => hashmap! {
                s("repoint") => 1,
            },
        });

        assert_eq!(test.by_repo_repoint("repo"), Some(1));
        assert_eq!(test.by_repo_repoint("repo2"), Some(4));

        test.update_by_repo_ints(&hashmap! {
            s(":override:") => hashmap! {
                s("repoint") => 4
            },
            s("repo") => hashmap! {
                s("repoint") => 1,
            },
        });

        assert_eq!(test.by_repo_repoint("repo"), Some(4));
        assert_eq!(test.by_repo_repoint("repo2"), Some(4));
    }

    #[test]
    fn update_by_repo_uint() {
        let test = TestTunables::default();

        assert_eq!(test.by_repo_repouint("repo"), None);
        assert_eq!(test.by_repo_repouint("repo2"), None);

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repouint") => 1,
            },
            s("repo2") => hashmap! {
                s("repouint") => 2,
            },
        });
        assert_eq!(test.by_repo_repouint("repo"), Some(1));
        assert_eq!(test.by_repo_repouint("repo2"), Some(2));

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repouint") => 3,
            },
        });
        assert_eq!(test.by_repo_repouint("repo"), Some(3));
        assert_eq!(test.by_repo_repouint("repo2"), None);

        test.update_by_repo_ints(&hashmap! {
            s(":default:") => hashmap! {
                s("repouint") => 4
            },
            s("repo") => hashmap! {
                s("repouint") => 1,
            },
        });

        assert_eq!(test.by_repo_repouint("repo"), Some(1));
        assert_eq!(test.by_repo_repouint("repo2"), Some(4));

        test.update_by_repo_ints(&hashmap! {
            s(":override:") => hashmap! {
                s("repouint") => 4
            },
            s("repo") => hashmap! {
                s("repouint") => 1,
            },
        });

        assert_eq!(test.by_repo_repouint("repo"), Some(4));
        assert_eq!(test.by_repo_repouint("repo2"), Some(4));
    }

    #[test]
    fn update_by_repo_two_ints() {
        let test = TestTunables::default();
        assert_eq!(test.by_repo_repoint("repo"), None);
        assert_eq!(test.by_repo_repoint2("repo"), None);

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repoint") => 1,
                s("repoint2") => 2,
            }
        });

        assert_eq!(test.by_repo_repoint("repo"), Some(1));
        assert_eq!(test.by_repo_repoint2("repo"), Some(2));

        test.update_by_repo_ints(&hashmap! {
            s("repo") => hashmap! {
                s("repoint") => 3
            }
        });

        assert_eq!(test.by_repo_repoint("repo"), Some(3));
        assert_eq!(test.by_repo_repoint2("repo"), None);
    }

    #[test]
    fn update_by_repo_vec_of_strings() {
        let test = TestTunables::default();
        assert_eq!(test.by_repo_repovecofstrings("repo"), None);

        test.update_by_repo_vec_of_strings(&hashmap! {
            s("repo") => hashmap! {
                s("unrelated") => vec![s("val1"), s("val2")],
            }
        });
        assert_eq!(test.by_repo_repovecofstrings("repo"), None);

        test.update_by_repo_vec_of_strings(&hashmap! {
            s("repo") => hashmap! {
                s("repovecofstrings") => vec![s("val1"), s("val2")],
            }
        });

        assert_eq!(
            test.by_repo_repovecofstrings("repo"),
            Some(vec![s("val1"), s("val2")])
        );

        test.update_by_repo_vec_of_strings(&hashmap! {
            s(":default:") => hashmap! {
                s("repovecofstrings") => vec![s("val3"), s("val4")],
            },
            s("repo") => hashmap! {
                s("repovecofstrings") => vec![s("val1"), s("val2")],
            },
        });

        assert_eq!(
            test.by_repo_repovecofstrings("repo"),
            Some(vec![s("val1"), s("val2")])
        );
        assert_eq!(
            test.by_repo_repovecofstrings("repo2"),
            Some(vec![s("val3"), s("val4")])
        );

        test.update_by_repo_vec_of_strings(&hashmap! {
            s(":override:") => hashmap! {
                s("repovecofstrings") => vec![s("val3"), s("val4")],
            },
            s("repo") => hashmap! {
                s("repovecofstrings") => vec![s("val1"), s("val2")],
            },
        });

        assert_eq!(
            test.by_repo_repovecofstrings("repo"),
            Some(vec![s("val3"), s("val4")])
        );
        assert_eq!(
            test.by_repo_repovecofstrings("repo2"),
            Some(vec![s("val3"), s("val4")])
        );
    }

    #[fbinit::test]
    async fn test_with_tunables_async(_fb: fbinit::FacebookInit) {
        let res = with_tunables_async(
            MononokeTunables {
                warm_bookmark_cache_poll_interval_ms: ArcSwapOption::from(Some(Arc::new(2))),
                ..MononokeTunables::default()
            },
            async {
                tunables()
                    .warm_bookmark_cache_poll_interval_ms()
                    .unwrap_or_default()
            }
            .boxed(),
        )
        .await;

        assert_eq!(res, 2);
    }
}
