/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
