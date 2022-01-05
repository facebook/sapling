/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::Condvar;
use parking_lot::Mutex;
use parking_lot::RwLock;
use parking_lot::RwLockUpgradableReadGuard;

use crate::CacheStats;
use crate::IoTimeSeries;
use crate::ProgressBar;

/// Data needed to render render multi-line progress.
///
/// There are 2 kinds of data:
/// - I/O time series. (ex. "Network [▁▂▄█▇▅▃▆] 3MB/s")
/// - Ordinary progress bars with "position" and "total".
///   (ex. "fetching files 123/456")
#[derive(Default, Clone, Debug)]
pub struct Registry {
    render_cond: Arc<(Mutex<bool>, Condvar)>,
    inner: Arc<RwLock<Inner>>,
}

macro_rules! impl_model {
    {
        $( $field:ident: $type:ty, )*
    } => {
        paste::paste! {
            #[derive(Default, Debug)]
            struct Inner {
                $( $field: Vec<Arc<$type>>, )*
            }

            impl Registry {
                $(
                    /// Register a model.
                    pub fn [< register_ $field >](&self, model: &Arc<$type>) {
                        tracing::debug!("registering {} {}", stringify!($type), model.topic());
                        let mut inner = self.inner.write();
                        inner.$field.push(model.clone());
                    }

                    /// List models registered.
                    pub fn [< list_ $field >](&self) -> Vec<Arc<$type>> {
                        self.inner.read().$field.clone()
                    }

                    /// Remove models that were dropped externally.
                    pub fn [< remove_orphan_ $field >](&self) -> usize {
                        let inner = self.inner.upgradable_read();
                        let orphan_count = inner
                            .$field
                            .iter()
                            .filter(|b| Arc::strong_count(b) == 1)
                            .count();
                        if orphan_count > 0 {
                            tracing::debug!(
                                "removing {} orphan {}",
                                orphan_count,
                                stringify!($type)
                            );
                            let mut inner = RwLockUpgradableReadGuard::upgrade(inner);
                            inner.$field = inner
                                .$field
                                .drain(..)
                                .filter(|b| Arc::strong_count(b) > 1)
                                .collect();
                        }
                        orphan_count
                    }
                )*

                /// Remove all registered models that are dropped externally.
                pub fn remove_orphan_models(&self) {
                    $( self.[< remove_orphan_ $field >](); )*
                }
            }
        }
    };
}

impl_model! {
    cache_stats: CacheStats,
    io_time_series: IoTimeSeries,
    progress_bar: ProgressBar,
}

impl Registry {
    /// The "main" progress registry in this process.
    pub fn main() -> &'static Self {
        static REGISTRY: Lazy<Registry> = Lazy::new(|| {
            tracing::debug!("main progress Registry initialized");
            Registry {
                render_cond: Arc::new((Mutex::new(false), Condvar::new())),
                ..Default::default()
            }
        });
        &REGISTRY
    }

    /// step/wait provide a mechanism for tests to step through
    /// rendering/handling of the registry in a controlled manner. The
    /// test calls step() which unblocks the wait()er. Then step()
    /// waits for the next wait() call, ensuring that the registry
    /// processing loop finished its iteration.
    pub fn step(&self) {
        let &(ref lock, ref var) = &*self.render_cond;
        let mut ready = lock.lock();
        *ready = true;
        var.notify_one();
        // Wait for wait() to notify us that it completed an iteration.
        var.wait(&mut ready);
    }

    /// See step().
    pub fn wait(&self) {
        let &(ref lock, ref var) = &*self.render_cond;
        let mut ready = lock.lock();
        if *ready {
            // We've come around to the next iteration's wait() call -
            // notify step() that we finished an iteration.
            *ready = false;
            var.notify_one();
        }
        // Wait for next step() call.
        var.wait(&mut ready);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_bar() {
        let registry = Registry::default();
        let topic = "fetching files";

        // Add 2 progress bars.
        let bar1 = {
            let bar = ProgressBar::new(topic.to_string(), 100, "files");
            bar.set_position(50);
            registry.register_progress_bar(&bar);
            bar
        };

        let bar2 = {
            let bar = ProgressBar::new(topic.to_string(), 200, "bytes");
            bar.increase_position(100);
            bar.set_message("a.txt".to_string());
            registry.register_progress_bar(&bar);
            bar
        };

        assert_eq!(registry.remove_orphan_progress_bar(), 0);
        assert_eq!(
            format!("{:?}", registry.list_progress_bar()),
            "[[fetching files 50/100 files, [fetching files 100/200 bytes a.txt]"
        );

        // Dropping a bar marks it as "completed" and affects aggregated_bars.
        drop(bar1);
        assert_eq!(registry.remove_orphan_progress_bar(), 1);
        assert_eq!(
            format!("{:?}", registry.list_progress_bar()),
            "[[fetching files 100/200 bytes a.txt]"
        );

        drop(bar2);
        assert_eq!(registry.remove_orphan_progress_bar(), 1);
        assert_eq!(format!("{:?}", registry.list_progress_bar()), "[]");
    }

    #[test]
    fn test_time_series() {
        let registry = Registry::default();
        let series1 = IoTimeSeries::new("Net", "requests");
        registry.register_io_time_series(&series1);
        let series2 = IoTimeSeries::new("Disk", "files");
        series2.populate_test_samples(1, 1, 1);
        registry.register_io_time_series(&series2);
        assert_eq!(
            format!("{:?}", registry.list_io_time_series()),
            "[Net [0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0, 0|0|0], Disk [0|0|0, 5000|300|1, 20000|1200|2, 45000|2700|3, 80000|4800|4, 125000|7500|5, 180000|10800|6, 245000|14700|7, 320000|19200|8, 405000|24300|9, 500000|30000|10, 605000|36300|11, 720000|43200|12, 845000|50700|13, 980000|58800|14, 1125000|67500|15]]"
        );
        drop(series1);
        drop(series2);
        assert_eq!(registry.remove_orphan_io_time_series(), 2);
        assert_eq!(format!("{:?}", registry.list_io_time_series()), "[]");
    }
}
