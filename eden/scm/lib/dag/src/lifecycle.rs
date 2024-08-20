/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::backtrace::Backtrace;
use std::fmt;
use std::io;
use std::io::Write;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::OnceLock;

/// Used as a struct field to provide a stable identity for debug logging purpose.
///
/// Differences from `VerLink`:
/// - Does not maintain a partial order.
/// - Crates a new `Id` on clone.
/// - Prints backtrace on new, clone, drop, if `RUST_BACKTRACE` and tracing level is "trace".
///
/// Example uses:
/// - When this type of struct gets created (new or cloned)?
/// - When this type of struct gets dropped?
/// - With custom logging, when an operation happens, what is the identity of this struct?
pub(crate) struct LifecycleId {
    id: usize,
    type_name: &'static str,
}

// Use a non-zero starting id to ease search.
static NEXT_LIFECYCLE_ID: AtomicUsize = AtomicUsize::new(2000);

impl LifecycleId {
    pub(crate) fn new<T>() -> Self {
        let type_name = std::any::type_name::<T>();
        // make it less verbose: "foo::bar::T" -> "T"
        let type_name = type_name.rsplit("::").next().unwrap_or(type_name);
        Self::new_with_type_name(type_name)
    }

    pub(crate) fn new_with_type_name(type_name: &'static str) -> Self {
        let id = NEXT_LIFECYCLE_ID.fetch_add(1, Ordering::AcqRel);
        tracing::debug!(type_name = type_name, id = id, "created");
        trace_print_short_backtrace();
        Self { id, type_name }
    }
}

impl fmt::Debug for LifecycleId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}", self.type_name, self.id)
    }
}

impl Clone for LifecycleId {
    fn clone(&self) -> Self {
        let type_name = self.type_name;
        let id = NEXT_LIFECYCLE_ID.fetch_add(1, Ordering::AcqRel);
        tracing::debug!(type_name = type_name, id = id, from_id = self.id, "cloned");
        trace_print_short_backtrace();
        Self { id, type_name }
    }
}

impl Drop for LifecycleId {
    fn drop(&mut self) {
        let type_name = self.type_name;
        let id = self.id;
        tracing::debug!(type_name = type_name, id = id, "dropped");
        trace_print_short_backtrace();
    }
}

pub(crate) fn trace_print_short_backtrace() {
    tracing::trace!(target: "dag::backtrace", "Backtrace:\n{:?}", short_backtrace(Backtrace::capture()));
}

/// Take first few lines of a (potentially long) backtrace.
fn short_backtrace(bt: Backtrace) -> TruncateLines {
    // Use TruncateLines to limit output and avoid deep symbol lookups.
    let mut out = TruncateLines::default();
    let _ = write!(out, "{}", bt);
    out
}

const DEFAULT_TRACEBACK_LINE_LIMIT: usize = 32;
const TRACEBACK_LIMIT_ENV_NAME: &str = "RUST_TRACEBACK_MAX_LINES";

/// Traceback line limit. `${RUST_TRACEBACK_LIMIT:-DEFAULT_TRACEBACK_LINE_LIMIT}`.
fn get_traceback_line_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    fn read_limit() -> Option<usize> {
        let limit = std::env::var(TRACEBACK_LIMIT_ENV_NAME).ok()?;
        let limit = limit.parse::<usize>().ok()?;
        Some(limit)
    }
    *LIMIT.get_or_init(|| read_limit().unwrap_or(DEFAULT_TRACEBACK_LINE_LIMIT))
}

/// `io::Write` implementation that only takes a few utf-8 lines.
#[derive(Default)]
struct TruncateLines(String, usize);

impl Write for TruncateLines {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.1 >= get_traceback_line_limit() {
            return Err(io::ErrorKind::Other.into());
        }
        if let Ok(s) = std::str::from_utf8(buf) {
            self.0.push_str(s);
            self.1 += buf.iter().filter(|&&b| b == b'\n').count();
            Ok(buf.len())
        } else {
            Ok(0)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl fmt::Debug for TruncateLines {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)?;
        if self.1 >= get_traceback_line_limit() {
            write!(
                f,
                "      (truncated, increase {} for more)",
                TRACEBACK_LIMIT_ENV_NAME
            )?;
        }
        Ok(())
    }
}
