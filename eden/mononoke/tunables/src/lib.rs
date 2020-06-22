/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cell::RefCell;
use std::ops::Deref;
use std::sync::Arc;
use std::thread;
use std::thread_local;
use std::time::Duration;

use anyhow::Result;
use arc_swap::ArcSwap;
use cached_config::ConfigHandle;
use futures::{future::poll_fn, Future, FutureExt};
use once_cell::sync::OnceCell;
use slog::{debug, warn, Logger};
use std::sync::atomic::{AtomicBool, AtomicI64};

use tunables_derive::Tunables;
use tunables_structs::Tunables as TunablesStruct;

static TUNABLES: OnceCell<MononokeTunables> = OnceCell::new();
const REFRESH_INTERVAL: Duration = Duration::from_secs(5);

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

// This type exists to simplify code generation in tunables-derive
pub type TunableString = ArcSwap<String>;

#[derive(Tunables, Default, Debug)]
pub struct MononokeTunables {
    mutation_advertise_for_infinitepush: AtomicBool,
    mutation_accept_for_infinitepush: AtomicBool,
    mutation_generate_for_draft: AtomicBool,
    warm_bookmark_cache_delay: AtomicI64,
    max_scuba_msg_length: AtomicI64,
    wishlist_read_qps: AtomicI64,
    wishlist_write_qps: AtomicI64,
    command_monitor_interval: AtomicI64,
    command_monitor_remote_logging: AtomicI64,
    // Log all getfiles/gettreepack requests for paths that start with prefix
    // in a particular repo
    undesired_path_repo_name_to_log: TunableString,
    undesired_path_prefix_to_log: TunableString,
    pushrebase_disable_rebased_commit_validation: AtomicBool,
    filenodes_disabled: AtomicBool,
    run_pushredirected_hooks_in_large_repo_killswitch: AtomicBool,
    skiplist_max_skips_without_yield: AtomicI64,
}

fn log_tunables(tunables: &TunablesStruct) -> String {
    serde_json::to_string(tunables)
        .unwrap_or_else(|e| format!("failed to serialize tunables: {}", e))
}

pub fn init_tunables_worker(
    logger: Logger,
    conf_handle: ConfigHandle<TunablesStruct>,
) -> Result<()> {
    let init_tunables = conf_handle.get();
    debug!(
        logger,
        "Initializing tunables: {}",
        log_tunables(&init_tunables)
    );
    update_tunables(init_tunables.clone())?;

    thread::Builder::new()
        .name("mononoke-tunables".into())
        .spawn(move || worker(conf_handle, init_tunables, logger))
        .expect("Can't spawn tunables updater");

    Ok(())
}

fn worker(
    config_handle: ConfigHandle<TunablesStruct>,
    init_tunables: Arc<TunablesStruct>,
    logger: Logger,
) {
    // Previous value of the tunables.  If we fail to update tunables,
    // this will be `None`.
    let mut old_tunables = Some(init_tunables);
    loop {
        // TODO: Instead of refreshing tunables every loop iteration,
        // update cached_config to notify us when our config has changed.
        let new_tunables = config_handle.get();
        if Some(&new_tunables) != old_tunables.as_ref() {
            debug!(
                logger,
                "Updating tunables, old: {}, new: {}",
                old_tunables
                    .as_deref()
                    .map(log_tunables)
                    .unwrap_or_else(|| String::from("unknown")),
                log_tunables(&new_tunables),
            );
            match update_tunables(new_tunables.clone()) {
                Ok(_) => {
                    old_tunables = Some(new_tunables);
                }
                Err(e) => {
                    warn!(logger, "Failed to refresh tunables: {}", e);
                    old_tunables = None;
                }
            }
        }

        thread::sleep(REFRESH_INTERVAL);
    }
}

fn update_tunables(new_tunables: Arc<TunablesStruct>) -> Result<()> {
    let tunables = tunables();
    tunables.update_bools(&new_tunables.killswitches);
    tunables.update_ints(&new_tunables.ints);
    tunables.update_strings(&new_tunables.strings);

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
    mut fut: Fut,
) -> impl Future<Output = Out> {
    let new_tunables = Arc::new(new_tunables);
    poll_fn(move |cx| {
        TUNABLES_OVERRIDE.with(|t| *t.borrow_mut() = Some(new_tunables.clone()));

        let res = fut.poll_unpin(cx);

        TUNABLES_OVERRIDE.with(|tunables| *tunables.borrow_mut() = None);

        res
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicBool;

    #[derive(Tunables, Default)]
    struct TestTunables {
        boolean: AtomicBool,
        num: AtomicI64,
        string: TunableString,
    }

    #[derive(Tunables, Default)]
    struct EmptyTunables {}

    #[test]
    fn test_override_tunables() {
        assert_eq!(tunables().get_wishlist_write_qps(), 0);

        let res = with_tunables(
            MononokeTunables {
                wishlist_write_qps: AtomicI64::new(2),
                ..MononokeTunables::default()
            },
            || tunables().get_wishlist_write_qps(),
        );

        assert_eq!(res, 2);
        assert_eq!(tunables().get_wishlist_write_qps(), 0);
    }

    #[test]
    fn test_empty_tunables() {
        let bools = HashMap::new();
        let ints = HashMap::new();
        let empty = EmptyTunables::default();

        empty.update_bools(&bools);
        empty.update_ints(&ints);
        empty.update_strings(&HashMap::new());
    }

    #[test]
    fn test_update_bool() {
        let mut d = HashMap::new();
        d.insert("boolean".to_string(), true);

        let test = TestTunables::default();
        assert_eq!(test.get_boolean(), false);
        test.update_bools(&d);
        assert_eq!(test.get_boolean(), true);
    }

    #[test]
    fn test_update_int() {
        let mut d = HashMap::new();
        d.insert("num".to_string(), 10);

        let test = TestTunables::default();
        assert_eq!(test.get_num(), 0);
        test.update_ints(&d);
        assert_eq!(test.get_num(), 10);
    }

    #[test]
    fn test_missing_int() {
        let mut d = HashMap::new();
        d.insert("missing".to_string(), 10);

        let test = TestTunables::default();
        assert_eq!(test.get_num(), 0);
        test.update_ints(&d);
        assert_eq!(test.get_num(), 0);
    }

    #[test]
    fn update_string() {
        let mut d = HashMap::new();
        d.insert("string".to_string(), "value".to_string());

        let test = TestTunables::default();
        assert_eq!(test.get_string().as_str(), "");
        test.update_strings(&d);
        assert_eq!(test.get_string().as_str(), "value");
    }

    #[fbinit::compat_test]
    async fn test_with_tunables_async(_fb: fbinit::FacebookInit) {
        let res = with_tunables_async(
            MononokeTunables {
                wishlist_write_qps: AtomicI64::new(2),
                ..MononokeTunables::default()
            },
            async { tunables().get_wishlist_write_qps() }.boxed(),
        )
        .await;

        assert_eq!(res, 2);
    }
}
