/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use srserver::RustThriftServerStatus;
use srserver::ThriftServerStatus;

/// Status of a Thrift server whose shutdown is driven by
/// [`MononokeApp`](crate::MononokeApp).
///
/// Reports `STOPPING` as soon as the app starts to quiesce, which is when the
/// termination signal arrives rather than when the shutdown grace period ends.
/// The Thrift runtime's own status only turns over once the server stops, by
/// which point the grace period this status exists to announce has already
/// elapsed.
#[derive(Clone)]
pub struct MononokeAppServerStatus {
    quiescing: Arc<AtomicBool>,
}

impl MononokeAppServerStatus {
    pub(crate) fn new(quiescing: Arc<AtomicBool>) -> Self {
        Self { quiescing }
    }
}

impl ThriftServerStatus for MononokeAppServerStatus {
    fn get_status(&self) -> RustThriftServerStatus {
        if self.quiescing.load(Ordering::Relaxed) {
            RustThriftServerStatus::STOPPING
        } else {
            RustThriftServerStatus::ALIVE
        }
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_reports_stopping_once_quiescing() {
        let quiescing = Arc::new(AtomicBool::new(false));
        let status = MononokeAppServerStatus::new(quiescing.clone());

        assert_eq!(
            status.get_status(),
            RustThriftServerStatus::ALIVE,
            "A server that has not been signalled should be ALIVE"
        );

        quiescing.store(true, Ordering::Relaxed);

        assert_eq!(
            status.get_status(),
            RustThriftServerStatus::STOPPING,
            "A server should report STOPPING for the whole grace period, not just once it stops"
        );
    }
}
