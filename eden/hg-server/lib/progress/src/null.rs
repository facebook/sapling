/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! No-op progress bar placeholders. Enables code that can optionally report
//! progress to unconditionally update the progress bar rather than using
//! conditionals at every callsite.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use anyhow::Result;

use crate::{ProgressBar, ProgressFactory, ProgressSpinner, Unit};

pub struct NullProgressFactory;

impl NullProgressFactory {
    pub fn arc() -> Arc<dyn ProgressFactory> {
        Arc::new(NullProgressFactory)
    }
}

impl ProgressFactory for NullProgressFactory {
    fn bar(&self, _message: &str, total: Option<u64>, _unit: Unit) -> Result<Box<dyn ProgressBar>> {
        Ok(Box::new(NullProgressBar::new(total)))
    }

    fn spinner(&self, _message: &str) -> Result<Box<dyn ProgressSpinner>> {
        Ok(Box::new(NullProgressSpinner))
    }
}

#[derive(Default)]
struct NullProgressBar {
    position: AtomicU64,
    total: AtomicU64,
}

impl NullProgressBar {
    fn new(total: Option<u64>) -> Self {
        Self {
            position: AtomicU64::new(0),
            // XXX: Use 0 to represent None so that we can just use an AtomicU64
            // to store the total. This is incorrect for cases where the caller
            // actually wants to set the total to 0, but that should be rare.
            total: AtomicU64::new(total.unwrap_or(0)),
        }
    }
}

impl ProgressBar for NullProgressBar {
    fn position(&self) -> Result<u64> {
        Ok(self.position.load(Ordering::Relaxed))
    }

    fn total(&self) -> Result<Option<u64>> {
        let total = self.total.load(Ordering::Relaxed);
        Ok(if total == 0 { None } else { Some(total) })
    }

    fn set(&self, pos: u64) -> Result<()> {
        self.position.store(pos, Ordering::Relaxed);
        Ok(())
    }

    fn set_total(&self, total: Option<u64>) -> Result<()> {
        self.total.store(total.unwrap_or(0), Ordering::Relaxed);
        Ok(())
    }

    fn increment(&self, delta: u64) -> Result<()> {
        let _ = self.position.fetch_add(delta, Ordering::Relaxed);
        Ok(())
    }

    fn set_message(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}

struct NullProgressSpinner;

impl ProgressSpinner for NullProgressSpinner {
    fn set_message(&self, _message: &str) -> Result<()> {
        Ok(())
    }
}
