// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use chashmap::CHashMap;
use serde::{Serialize, Serializer};
use std::sync::Arc;

use scuba_ext::ScubaSampleBuilder;
use slog::{o, Logger, OwnedKV, SendSyncRefUnwindSafeKV};
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct CoreContext {
    inner: Arc<Inner>,
}

#[derive(Debug, Clone)]
pub struct PerfCounters {
    // A wrapper around a concurrent HashMap that allows for
    // tracking of arbitrary counters.
    counters: CHashMap<&'static str, i64>,
}

impl Serialize for PerfCounters {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_map(self.counters.clone().into_iter())
    }
}

impl PerfCounters {
    pub fn new() -> Self {
        Self {
            counters: CHashMap::new(),
        }
    }

    pub fn change_counter<F>(&self, name: &'static str, default: i64, update: F)
    where
        F: FnOnce(&mut i64),
    {
        self.counters.upsert(name, || default, update);
    }

    pub fn set_counter(&self, name: &'static str, val: i64) {
        self.counters.insert(name, val);
    }

    pub fn increment_counter(&self, name: &'static str) {
        self.add_to_counter(name, 1);
    }

    pub fn decrement_counter(&self, name: &'static str) {
        self.add_to_counter(name, -1);
    }

    pub fn add_to_counter(&self, name: &'static str, val: i64) {
        self.change_counter(name, val, |old| *old += val);
    }

    pub fn set_max_counter(&self, name: &'static str, val: i64) {
        self.change_counter(name, val, |old| {
            if val > *old {
                *old = val
            }
        });
    }
}

#[derive(Debug, Clone)]
struct Inner {
    session: Uuid,
    logger: Logger,
    scuba: ScubaSampleBuilder,
    // Logging some prod wireproto requests to scribe so that they can be later replayed on
    // shadow tier.
    wireproto_scribe_category: Option<String>,
    trace: TraceContext,
    perf_counters: PerfCounters,
    user_unix_name: Option<String>,
    ssh_env_vars: SshEnvVars,
}

impl CoreContext {
    pub fn new(
        session: Uuid,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        wireproto_scribe_category: Option<String>,
        trace: TraceContext,
        user_unix_name: Option<String>,
        ssh_env_vars: SshEnvVars,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                session,
                logger,
                scuba,
                wireproto_scribe_category,
                trace,
                perf_counters: PerfCounters::new(),
                user_unix_name,
                ssh_env_vars,
            }),
        }
    }

    pub fn new_with_logger(logger: Logger) -> Self {
        Self::new(
            Uuid::new_v4(),
            logger,
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
        )
    }

    pub fn with_logger_kv<T>(&self, values: OwnedKV<T>) -> Self
    where
        T: SendSyncRefUnwindSafeKV + 'static,
    {
        Self {
            inner: Arc::new(Inner {
                session: self.inner.session.clone(),
                logger: self.inner.logger.new(values),
                scuba: self.inner.scuba.clone(),
                wireproto_scribe_category: self.inner.wireproto_scribe_category.clone(),
                trace: self.inner.trace.clone(),
                perf_counters: self.inner.perf_counters.clone(),
                user_unix_name: self.inner.user_unix_name.clone(),
                ssh_env_vars: self.inner.ssh_env_vars.clone(),
            }),
        }
    }

    pub fn with_scuba_initialization<F>(&self, init: F) -> Self
    where
        F: FnOnce(ScubaSampleBuilder) -> ScubaSampleBuilder,
    {
        Self {
            inner: Arc::new(Inner {
                session: self.inner.session.clone(),
                logger: self.inner.logger.clone(),
                scuba: init(self.inner.scuba.clone()),
                wireproto_scribe_category: self.inner.wireproto_scribe_category.clone(),
                trace: self.inner.trace.clone(),
                perf_counters: self.inner.perf_counters.clone(),
                user_unix_name: self.inner.user_unix_name.clone(),
                ssh_env_vars: self.inner.ssh_env_vars.clone(),
            }),
        }
    }

    pub fn test_mock() -> Self {
        Self::new(
            Uuid::new_v4(),
            Logger::root(::slog::Discard, o!()),
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
            None,
            SshEnvVars::default(),
        )
    }

    pub fn session(&self) -> &Uuid {
        &self.inner.session
    }
    pub fn logger(&self) -> &Logger {
        &self.inner.logger
    }
    pub fn scuba(&self) -> &ScubaSampleBuilder {
        &self.inner.scuba
    }
    pub fn wireproto_scribe_category(&self) -> &Option<String> {
        &self.inner.wireproto_scribe_category
    }
    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }
    pub fn perf_counters(&self) -> &PerfCounters {
        &self.inner.perf_counters
    }
    pub fn user_unix_name(&self) -> &Option<String> {
        &self.inner.user_unix_name
    }
    pub fn ssh_env_vars(&self) -> &SshEnvVars {
        &self.inner.ssh_env_vars
    }
    pub fn is_quicksand(&self) -> bool {
        if let Some(ref ssh_cert_principals) = self.ssh_env_vars().ssh_cert_principals {
            ssh_cert_principals.contains("quicksand")
        } else {
            false
        }
    }
}
