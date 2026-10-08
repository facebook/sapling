/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Dynamic control tracing `Layer`s.
//!
//! ```ignore
//! let layer = layers(vec![layer1.boxed(), layer2.boxed(), layer3.boxed()]);
//! Registry::default().with(layer);
//! ```
//!
//! is similar to:
//!
//! ```ignore
//! Registry::default().with(layer1).with(layer2).with(layer3)
//! ```
//!
//! but does not force static types (which makes conditional layers awkward).
//! See also https://github.com/tokio-rs/tracing/issues/2499
//!
//! # `tracing_subscriber`'s filtering pitfall
//!
//! `Layer` without `with_filter` can be slow. For example, this compiles,
//! but `Subscriber::register_callsite` will always return "sometimes"
//! (due to `Layer` implementation detail), causing `enabled` to be called
//! every time, therefore slow.
//!
//! ```ignore
//! Registry::default().with(CustomLayer(vec![
//!     layer_a.boxed(),
//!     layer_b.boxed(),
//! ]))
//! ```
//!
//! Must use `with_filter` to gain fast paths:
//!
//! ```ignore
//! Registry::default().with(CustomLayer(vec![
//!     layer_a.with_filter(filter_a).boxed(),
//!     layer_b.with_filter(filter_b).boxed(),
//! ]).with_filter(some_filter))
//! ```
//!
//! The `layers` function in this crate handles the top-level `with_filter` part
//! automatically.

use std::sync::Arc;
use std::sync::OnceLock;

use tracing::Dispatch;
use tracing::Event;
use tracing::Metadata;
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing::span;
use tracing::subscriber::Interest;
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::layer::Filter;
use tracing_subscriber::registry::LookupSpan;

/// Shorthand for a boxed, type-erased `Layer`.
pub type BoxedLayer<S> = Box<dyn Layer<S> + Send + Sync + 'static>;

/// Shared read-only access to the layer list for both `DynLayer` and `UnionFilter`.
///
/// `DynLayer` and `UnionFilter` are separate structs (layer vs filter) joined
/// by `Filtered`, so they cannot hold references to each other. Instead they
/// share layers through this `Arc`. The `OnceLock` allows `DynLayer::on_layer`
/// to call each inner layer's `on_layer(&mut self, …)` while it still has
/// exclusive ownership, then move the `Vec` into the lock for shared reading.
type SharedLayers<S> = Arc<OnceLock<Vec<BoxedLayer<S>>>>;

/// Creates a dynamically-composed layer from a list of per-layer-filtered layers.
///
/// Returns a single `impl Layer<S>` backed by a `Filtered<DynLayer, UnionFilter>`.
/// See the [module docs](self) for usage and constraints.
pub fn layers<S>(layers: Vec<BoxedLayer<S>>) -> impl Layer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a> + 'static,
{
    let shared: SharedLayers<S> = Arc::new(OnceLock::new());
    let filter = UnionFilter(shared.clone());
    let layer = DynLayer { layers, shared };
    layer.with_filter(filter)
}

// -- Union filter ------------------------------------------------------------
//
// Always returns `Interest::never()` from `callsite_enabled`, but first
// iterates inner layers' `register_callsite` so their per-layer filters
// record interest in the FILTERING thread-local. This makes the union
// filter's FilterId contribute "never" to the aggregate, allowing
// `Registry::take_interest()` to return `Interest::never()` when all inner
// filters also say "never".
//
// For dispatch, `enabled` always returns `true` (so the outer `Filtered`'s
// `did_enable` passes) while also iterating inner layers' `enabled` to set
// their FILTERING bits for their own `did_enable` checks.

struct UnionFilter<S>(SharedLayers<S>);

impl<S: Subscriber> Filter<S> for UnionFilter<S> {
    fn callsite_enabled(&self, meta: &'static Metadata<'static>) -> Interest {
        let layers = self.0.get().expect("on_layer not called");
        for layer in layers.iter() {
            layer.register_callsite(meta);
        }
        Interest::never()
    }

    fn enabled(&self, meta: &Metadata<'_>, cx: &Context<'_, S>) -> bool {
        // Iterate inner layers' `enabled` for per-layer filter side effects
        // (sets FILTERING bits checked by `did_enable` in `on_event` etc.).
        let layers = self.0.get().expect("on_layer not called");
        for layer in layers.iter() {
            layer.enabled(meta, cx.clone());
        }
        true
    }
}

// -- Dynamic layer -----------------------------------------------------------
//
// Dispatches all `Layer` callbacks to the shared list of inner layers.
// Does NOT override `register_callsite` — the default calls `self.enabled`,
// which is fine. `UnionFilter::callsite_enabled` handles calling inner
// layers' `register_callsite` to avoid double-calling (and `Filtered`
// skips calling `Layer::register_callsite` when the filter says "never").

struct DynLayer<S> {
    /// Owned initially; moved to `shared` in `on_layer`.
    layers: Vec<BoxedLayer<S>>,
    shared: SharedLayers<S>,
}

impl<S> DynLayer<S> {
    fn get(&self) -> &Vec<BoxedLayer<S>> {
        self.shared.get().expect("on_layer not called")
    }
}

impl<S: Subscriber> Layer<S> for DynLayer<S> {
    fn on_register_dispatch(&self, subscriber: &Dispatch) {
        for layer in self.get() {
            layer.on_register_dispatch(subscriber);
        }
    }

    fn on_layer(&mut self, subscriber: &mut S) {
        for layer in &mut self.layers {
            layer.on_layer(subscriber);
        }
        let _ = self.shared.set(std::mem::take(&mut self.layers));
    }

    // register_callsite: intentionally NOT overridden. See module comment.

    fn enabled(&self, metadata: &Metadata<'_>, ctx: Context<'_, S>) -> bool {
        let mut enabled = false;
        for layer in self.get() {
            enabled |= layer.enabled(metadata, ctx.clone());
        }
        enabled
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_new_span(attrs, id, ctx.clone());
        }
    }

    fn max_level_hint(&self) -> Option<LevelFilter> {
        let layers = self.shared.get()?;
        let mut max_level = LevelFilter::ERROR;
        for layer in layers.iter() {
            max_level = std::cmp::max(layer.max_level_hint()?, max_level);
        }
        Some(max_level)
    }

    fn on_record(&self, span: &span::Id, values: &span::Record<'_>, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_record(span, values, ctx.clone());
        }
    }

    fn on_follows_from(&self, span: &span::Id, follows: &span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_follows_from(span, follows, ctx.clone());
        }
    }

    fn event_enabled(&self, event: &Event<'_>, ctx: Context<'_, S>) -> bool {
        let mut enabled = false;
        for layer in self.get() {
            enabled |= layer.event_enabled(event, ctx.clone());
        }
        enabled
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_event(event, ctx.clone());
        }
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_enter(id, ctx.clone());
        }
    }

    fn on_exit(&self, id: &span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_exit(id, ctx.clone());
        }
    }

    fn on_close(&self, id: span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_close(id.clone(), ctx.clone());
        }
    }

    fn on_id_change(&self, old: &span::Id, new: &span::Id, ctx: Context<'_, S>) {
        for layer in self.get() {
            layer.on_id_change(old, new, ctx.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;

    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;

    // Callsite interest combines all live subscribers, even thread-local ones.
    // Keep each test's subscribers isolated until they have been dropped.
    static SUBSCRIBER_LOCK: Mutex<()> = Mutex::new(());

    struct CountingLayer(Arc<AtomicUsize>);

    impl<S: Subscriber> Layer<S> for CountingLayer {
        fn on_event(&self, _event: &Event<'_>, _ctx: Context<'_, S>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// A pass-through filter that accepts everything.
    struct AcceptAll;

    impl<S: Subscriber> Filter<S> for AcceptAll {
        fn enabled(&self, _: &Metadata<'_>, _: &Context<'_, S>) -> bool {
            true
        }

        fn callsite_enabled(&self, _: &'static Metadata<'static>) -> Interest {
            Interest::always()
        }
    }

    #[test]
    fn test_dispatches_to_all_layers() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        let c1 = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::new(AtomicUsize::new(0));

        let layer = layers(vec![
            CountingLayer(c1.clone()).with_filter(AcceptAll).boxed(),
            CountingLayer(c2.clone()).with_filter(AcceptAll).boxed(),
        ]);

        let _sub = tracing::subscriber::set_default(Registry::default().with(layer));

        tracing::info!("one");
        tracing::info!("two");

        assert_eq!(c1.load(Ordering::Relaxed), 2);
        assert_eq!(c2.load(Ordering::Relaxed), 2);
    }

    /// Verifies the documented performance property: when all inner
    /// per-layer filters return `Interest::never()`, the callsite is
    /// fully disabled — `enabled()` is never called on the hot path.
    #[test]
    fn test_never_callsite_interest_skips_enabled() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        struct RejectAll(Arc<AtomicUsize>);

        impl<S: Subscriber> Filter<S> for RejectAll {
            fn callsite_enabled(&self, _: &'static Metadata<'static>) -> Interest {
                Interest::never()
            }
            fn enabled(&self, _: &Metadata<'_>, _: &Context<'_, S>) -> bool {
                self.0.fetch_add(1, Ordering::Relaxed);
                false
            }
        }

        let enabled_calls = Arc::new(AtomicUsize::new(0));
        let event_count = Arc::new(AtomicUsize::new(0));

        let layer = layers(vec![
            CountingLayer(event_count.clone())
                .with_filter(RejectAll(enabled_calls.clone()))
                .boxed(),
        ]);

        let _sub = tracing::subscriber::set_default(Registry::default().with(layer));

        for _ in 0..10 {
            tracing::info!("rejected at callsite level");
        }

        assert_eq!(
            enabled_calls.load(Ordering::Relaxed),
            0,
            "enabled() should not be called when callsite interest is 'never'"
        );
        assert_eq!(event_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_independent_filters() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        let c_info = Arc::new(AtomicUsize::new(0));
        let c_all = Arc::new(AtomicUsize::new(0));

        let layer = layers(vec![
            CountingLayer(c_info.clone())
                .with_filter(LevelFilter::INFO)
                .boxed(),
            CountingLayer(c_all.clone())
                .with_filter(LevelFilter::TRACE)
                .boxed(),
        ]);

        let _sub = tracing::subscriber::set_default(Registry::default().with(layer));

        tracing::info!("visible to both");
        tracing::trace!("visible to trace-level only");

        assert_eq!(c_info.load(Ordering::Relaxed), 1);
        assert_eq!(c_all.load(Ordering::Relaxed), 2);
    }
}
