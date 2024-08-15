/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Registry for "publisher / subscriber", "signal / slot", "event listener / emitter"
//! useful to decouple logic from different libraries.
//!
//! Check [`PubSub`] for the main structure.

mod pubsub;

use std::any::Any;
use std::sync::OnceLock;

pub use pubsub::PubSub;
pub use pubsub::SubscribedHandle;

/// Delegates to the global `PubSub`'s `subscribe`.
pub fn subscribe(
    name: impl ToString,
    func: impl (Fn(&dyn Any) -> anyhow::Result<()>) + Send + Sync + 'static,
) -> pubsub::SubscribedHandle {
    global_pubsub().subscribe(name, func)
}

/// Delegates to the global `PubSub`'s `publish`.
pub fn publish(name: &str, value: &dyn Any) -> anyhow::Result<()> {
    global_pubsub().publish(name, value)
}

fn global_pubsub() -> &'static PubSub {
    static PUBSUB: OnceLock<PubSub> = OnceLock::new();
    PUBSUB.get_or_init(Default::default)
}
