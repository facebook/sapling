/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Rust implementation surface for a channel-pipeline tail endpoint.

use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;

use crate::BytesPtr;
use crate::CallbackContext;
use crate::HandlerResult;
use crate::RustTypeErasedBox;

/// A Rust-backed terminal endpoint for one channel pipeline.
///
/// Unlike [`crate::RustHandler`], a tail has no next inbound handler. Its
/// `on_read` implementation must consume the message and either write a
/// response through `context` or retain an owned continuation for asynchronous
/// completion.
///
/// Endpoint instances need not be `Send`: they are constructed, invoked, and
/// destroyed on their pipeline's EventBase. Futures spawned from `on_read`
/// remain confined to that EventBase as well.
pub trait RustTailEndpoint: 'static {
    fn on_read(
        &mut self,
        context: &mut CallbackContext<'_>,
        message: RustTypeErasedBox<'_>,
    ) -> HandlerResult;

    fn on_exception(&mut self) {}
    fn on_write_ready(&mut self) {}
    fn on_pipeline_active(&mut self) {}
    fn on_pipeline_inactive(&mut self) {}
    fn handler_added(&mut self) {}
    fn handler_removed(&mut self) {}
}

/// Type-erased Rust endpoint owned by the C++ tail shim.
pub struct RustTailEndpointOpaque {
    pub(crate) inner: Box<dyn RustTailEndpoint>,
}

// SAFETY: this is the same opaque Rust type declared by the owning CXX bridge.
// It crosses the boundary only behind `rust::Box`; C++ never observes its
// layout or moves the contained endpoint between threads.
unsafe impl cxx::ExternType for RustTailEndpointOpaque {
    type Id = cxx::type_id!("channel_pipeline_rust::RustTailEndpointOpaque");
    type Kind = cxx::kind::Opaque;
}

/// Type-erases an EventBase-local tail endpoint for C++ ownership.
pub fn box_tail_endpoint(endpoint: impl RustTailEndpoint) -> Box<RustTailEndpointOpaque> {
    Box::new(RustTailEndpointOpaque {
        inner: Box::new(endpoint),
    })
}

static TEST_READS: AtomicU32 = AtomicU32::new(0);
static TEST_EXCEPTIONS: AtomicU32 = AtomicU32::new(0);
static TEST_WRITE_READY: AtomicU32 = AtomicU32::new(0);
static TEST_ADDED: AtomicU32 = AtomicU32::new(0);
static TEST_ACTIVE: AtomicU32 = AtomicU32::new(0);
static TEST_INACTIVE: AtomicU32 = AtomicU32::new(0);
static TEST_REMOVED: AtomicU32 = AtomicU32::new(0);
static TEST_QUEUED_COMPLETIONS: AtomicU32 = AtomicU32::new(0);

pub(crate) struct EchoTestTail;

#[derive(Default)]
pub(crate) struct QueuedTestTail {
    task: Option<crate::LocalTaskHandle>,
}

impl RustTailEndpoint for EchoTestTail {
    fn on_read(
        &mut self,
        context: &mut CallbackContext<'_>,
        mut message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        TEST_READS.fetch_add(1, Ordering::Relaxed);
        context.fire_write(message.take::<BytesPtr>())
    }

    fn on_exception(&mut self) {
        TEST_EXCEPTIONS.fetch_add(1, Ordering::Relaxed);
    }

    fn on_write_ready(&mut self) {
        TEST_WRITE_READY.fetch_add(1, Ordering::Relaxed);
    }

    fn on_pipeline_active(&mut self) {
        TEST_ACTIVE.fetch_add(1, Ordering::Relaxed);
    }

    fn on_pipeline_inactive(&mut self) {
        TEST_INACTIVE.fetch_add(1, Ordering::Relaxed);
    }

    fn handler_added(&mut self) {
        TEST_ADDED.fetch_add(1, Ordering::Relaxed);
    }

    fn handler_removed(&mut self) {
        TEST_REMOVED.fetch_add(1, Ordering::Relaxed);
    }
}

impl RustTailEndpoint for QueuedTestTail {
    fn on_read(
        &mut self,
        context: &mut CallbackContext<'_>,
        message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        match context.spawn_deferred_read_queued(message, std::future::ready(()), |deferred, ()| {
            drop(deferred);
            TEST_QUEUED_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
        }) {
            Ok(handle) => {
                self.task = Some(handle);
                HandlerResult::Success
            }
            Err(result) => result,
        }
    }
}

pub(crate) fn reset_test_counts() {
    for counter in [
        &TEST_READS,
        &TEST_EXCEPTIONS,
        &TEST_WRITE_READY,
        &TEST_ADDED,
        &TEST_ACTIVE,
        &TEST_INACTIVE,
        &TEST_REMOVED,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
    TEST_QUEUED_COMPLETIONS.store(0, Ordering::Relaxed);
}

pub(crate) fn test_counts() -> [u32; 7] {
    [
        TEST_READS.load(Ordering::Relaxed),
        TEST_EXCEPTIONS.load(Ordering::Relaxed),
        TEST_WRITE_READY.load(Ordering::Relaxed),
        TEST_ADDED.load(Ordering::Relaxed),
        TEST_ACTIVE.load(Ordering::Relaxed),
        TEST_INACTIVE.load(Ordering::Relaxed),
        TEST_REMOVED.load(Ordering::Relaxed),
    ]
}

pub(crate) fn queued_test_completions() -> u32 {
    TEST_QUEUED_COMPLETIONS.load(Ordering::Relaxed)
}
