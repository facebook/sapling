/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::channel::oneshot::Receiver;
use futures_stats::StreamStats;
use gotham_derive::StateData;

#[derive(StateData)]
pub struct PendingStreamStats {
    stats: Option<Receiver<StreamStats>>,
}

impl PendingStreamStats {
    pub fn none() -> Self {
        Self { stats: None }
    }

    pub fn deferred(receiver: Receiver<StreamStats>) -> Self {
        Self {
            stats: Some(receiver),
        }
    }

    pub async fn finish(self) -> Option<StreamStats> {
        match self.stats {
            Some(receiver) => receiver.await.ok(),
            None => None,
        }
    }
}
