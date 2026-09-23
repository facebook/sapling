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

use std::collections::VecDeque;
use std::num::NonZeroU64;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

use crate::BytesPtr;
use crate::CallbackContext;
use crate::ContextHandle;
use crate::HandlerResult;
use crate::LocalPipelineContext;
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
///
/// `on_read_outcome` is an additive output hook whose default delegates to the
/// legacy `on_read` callback. `on_pipeline_active_write` and
/// `on_write_ready_write` remain the lifecycle output hooks. A returned
/// [`TailWrite`] may be an IOBuf chain containing several encoded frames; the
/// C++ shim submits it only after the Rust callback returns.
pub trait RustTailEndpoint: 'static {
    fn on_read(
        &mut self,
        context: &mut CallbackContext<'_>,
        message: RustTypeErasedBox<'_>,
    ) -> HandlerResult;

    fn on_read_outcome(
        &mut self,
        context: &mut CallbackContext<'_>,
        message: RustTypeErasedBox<'_>,
    ) -> TailReadOutcome {
        TailReadOutcome::new(self.on_read(context, message))
    }

    fn on_exception(&mut self) {}
    fn on_write_ready(&mut self) {}
    fn on_write_ready_write(&mut self) -> Option<TailWrite> {
        self.on_write_ready();
        None
    }
    fn on_write_ready_outcome(&mut self) -> TailWriteOutcome {
        TailWriteOutcome::new(self.on_write_ready_write())
    }

    /// Receives the actual transport result associated with `token`.
    ///
    /// This runs after the callback that produced the token has returned. If
    /// retained pipeline access synchronously causes readiness or lifecycle
    /// notifications here, the C++ adapter latches their Rust callbacks until
    /// this borrow ends; no nested callback may alias this `&mut self`.
    fn on_write_result(&mut self, _token: TailWriteFeedbackToken, _result: HandlerResult) {}
    fn on_pipeline_active(&mut self) {}
    fn on_pipeline_active_write(&mut self) -> Option<TailWrite> {
        self.on_pipeline_active();
        None
    }
    fn on_pipeline_inactive(&mut self) {}
    fn handler_added(&mut self) {}
    fn handler_removed(&mut self) {}
}

/// The semantic read result plus optional immediate wire output and feedback.
///
/// The output and token are independent: a token without output receives
/// [`HandlerResult::Success`] after the initial callback borrow ends.
#[derive(Debug)]
pub struct TailReadOutcome {
    result: HandlerResult,
    write: Option<TailWrite>,
    feedback_token: Option<TailWriteFeedbackToken>,
}

impl TailReadOutcome {
    /// Creates an outcome with no immediate output or feedback.
    pub fn new(result: HandlerResult) -> Self {
        Self {
            result,
            write: None,
            feedback_token: None,
        }
    }

    /// Adds one owned IOBuf chain to submit after the callback returns.
    pub fn with_write(mut self, write: TailWrite) -> Self {
        self.write = Some(write);
        self
    }

    /// Requests post-write feedback after the callback returns.
    pub fn with_feedback(mut self, token: TailWriteFeedbackToken) -> Self {
        self.feedback_token = Some(token);
        self
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        HandlerResult,
        Option<TailWrite>,
        Option<TailWriteFeedbackToken>,
    ) {
        (self.result, self.write, self.feedback_token)
    }
}

/// Optional output and feedback produced by `on_write_ready`.
#[derive(Debug)]
pub struct TailWriteOutcome {
    write: Option<TailWrite>,
    feedback_token: Option<TailWriteFeedbackToken>,
}

impl TailWriteOutcome {
    /// Creates a write-ready outcome without feedback.
    pub fn new(write: Option<TailWrite>) -> Self {
        Self {
            write,
            feedback_token: None,
        }
    }

    /// Requests feedback even when the callback produced no bytes.
    pub fn with_feedback(mut self, token: TailWriteFeedbackToken) -> Self {
        self.feedback_token = Some(token);
        self
    }

    pub(crate) fn into_parts(self) -> (Option<TailWrite>, Option<TailWriteFeedbackToken>) {
        (self.write, self.feedback_token)
    }
}

/// Allocation-free token returned unchanged with a submitted write's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TailWriteFeedbackToken(NonZeroU64);

impl TailWriteFeedbackToken {
    /// Creates a token, returning zero unchanged because it is the FFI sentinel.
    pub fn try_new(value: u64) -> Result<Self, u64> {
        NonZeroU64::new(value).map(Self).ok_or(value)
    }

    /// Returns the caller-defined token value.
    pub fn get(self) -> u64 {
        self.0.get()
    }
}

/// A non-null IOBuf chain returned by a tail callback.
///
/// Construction rejects null ownership, keeping `None` unambiguously reserved
/// for callbacks that have no write to submit.
#[derive(Debug)]
pub struct TailWrite(BytesPtr);

impl TailWrite {
    /// Wraps a non-null write or returns the invalid input unchanged.
    pub fn try_new(bytes: BytesPtr) -> Result<Self, BytesPtr> {
        if bytes.is_null() {
            Err(bytes)
        } else {
            Ok(Self(bytes))
        }
    }

    pub(crate) fn into_bytes(self) -> BytesPtr {
        self.0
    }
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
static WRITE_TEST_EXCEPTIONS: AtomicU32 = AtomicU32::new(0);
static WRITE_TEST_INACTIVE: AtomicU32 = AtomicU32::new(0);
static WRITE_TEST_REMOVED: AtomicU32 = AtomicU32::new(0);
static WRITE_TEST_REJECTED_NULL: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_READS: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_FEEDBACK_SUCCESS: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_FEEDBACK_BACKPRESSURE: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_FEEDBACK_ERROR: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_WRITE_READY: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_EXCEPTIONS: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_INACTIVE: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_REMOVED: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_ACTIVE: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_REENTRANCY: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_SEQUENCE: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_FEEDBACK_COUNT: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_LAST_TOKEN: AtomicU64 = AtomicU64::new(0);
static OUTCOME_TEST_FEEDBACK_ORDERS: [AtomicU32; 3] =
    [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];
static OUTCOME_TEST_READY_ORDERS: [AtomicU32; 2] = [AtomicU32::new(0), AtomicU32::new(0)];
static OUTCOME_TEST_EXCEPTION_ORDER: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_INACTIVE_ORDER: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_REMOVED_ORDER: AtomicU32 = AtomicU32::new(0);
static OUTCOME_TEST_ACTIVE_ORDER: AtomicU32 = AtomicU32::new(0);

pub(crate) struct EchoTestTail;

#[derive(Default)]
pub(crate) struct QueuedTestTail {
    task: Option<crate::LocalTaskHandle>,
}

pub(crate) struct LifecycleWriteTestTail {
    write: Option<TailWrite>,
    on_activation: bool,
}

pub(crate) struct PanickingLifecycleWriteTestTail {
    on_activation: bool,
}

impl PanickingLifecycleWriteTestTail {
    pub(crate) fn new(on_activation: bool) -> Self {
        Self { on_activation }
    }
}

pub(crate) struct ReadOutcomeTestTail {
    result: HandlerResult,
    read_write: Option<TailWrite>,
    read_feedback: Option<TailWriteFeedbackToken>,
    ready_outcomes: VecDeque<TailWriteOutcome>,
    lifecycle_context: Option<LocalPipelineContext>,
    feedback_context: Option<ContextHandle>,
    feedback_write: Option<BytesPtr>,
    activation_write: Option<TailWrite>,
    activation_armed: bool,
    close_on_read: bool,
    close_on_ready: bool,
    close_on_feedback: bool,
    callback_active: bool,
}

pub(crate) struct ReturnedWriteBenchTail;
pub(crate) struct LegacyWriteBenchTail;

impl ReadOutcomeTestTail {
    pub(crate) fn new(
        result: HandlerResult,
        read_write: Option<TailWrite>,
        read_feedback: Option<TailWriteFeedbackToken>,
        ready_outcomes: VecDeque<TailWriteOutcome>,
        feedback_write: Option<BytesPtr>,
        activation_write: Option<TailWrite>,
        close_on_read: bool,
        close_on_ready: bool,
        close_on_feedback: bool,
    ) -> Self {
        Self {
            result,
            read_write,
            read_feedback,
            ready_outcomes,
            lifecycle_context: None,
            feedback_context: None,
            feedback_write,
            activation_write,
            activation_armed: false,
            close_on_read,
            close_on_ready,
            close_on_feedback,
            callback_active: false,
        }
    }

    fn enter_callback(&mut self) {
        if self.callback_active {
            OUTCOME_TEST_REENTRANCY.fetch_add(1, Ordering::Relaxed);
        }
        self.callback_active = true;
    }

    fn leave_callback(&mut self) {
        self.callback_active = false;
    }
}

fn record_outcome_order(destination: &AtomicU32) {
    destination.store(
        OUTCOME_TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1,
        Ordering::Relaxed,
    );
}

impl LifecycleWriteTestTail {
    pub(crate) fn new(write: BytesPtr, on_activation: bool) -> Self {
        let write = match TailWrite::try_new(write) {
            Ok(write) => Some(write),
            Err(_) => {
                WRITE_TEST_REJECTED_NULL.fetch_add(1, Ordering::Relaxed);
                None
            }
        };
        Self {
            write,
            on_activation,
        }
    }

    fn take_write(&mut self) -> Option<TailWrite> {
        self.write.take()
    }
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

impl RustTailEndpoint for LifecycleWriteTestTail {
    fn on_read(
        &mut self,
        _context: &mut CallbackContext<'_>,
        _message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        HandlerResult::Success
    }

    fn on_pipeline_active_write(&mut self) -> Option<TailWrite> {
        self.on_activation.then(|| self.take_write()).flatten()
    }

    fn on_write_ready_write(&mut self) -> Option<TailWrite> {
        (!self.on_activation).then(|| self.take_write()).flatten()
    }

    fn on_exception(&mut self) {
        WRITE_TEST_EXCEPTIONS.fetch_add(1, Ordering::Relaxed);
    }

    fn on_pipeline_inactive(&mut self) {
        WRITE_TEST_INACTIVE.fetch_add(1, Ordering::Relaxed);
    }

    fn handler_removed(&mut self) {
        WRITE_TEST_REMOVED.fetch_add(1, Ordering::Relaxed);
    }
}

impl RustTailEndpoint for PanickingLifecycleWriteTestTail {
    fn on_read(
        &mut self,
        _context: &mut CallbackContext<'_>,
        _message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        HandlerResult::Success
    }

    fn on_pipeline_active_write(&mut self) -> Option<TailWrite> {
        assert!(!self.on_activation, "activation write panic");
        None
    }

    fn on_write_ready_outcome(&mut self) -> TailWriteOutcome {
        assert!(self.on_activation, "write-ready write panic");
        TailWriteOutcome::new(None)
    }
}

impl RustTailEndpoint for ReadOutcomeTestTail {
    fn on_read(
        &mut self,
        _context: &mut CallbackContext<'_>,
        _message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        HandlerResult::Error
    }

    fn on_read_outcome(
        &mut self,
        context: &mut CallbackContext<'_>,
        _message: RustTypeErasedBox<'_>,
    ) -> TailReadOutcome {
        self.enter_callback();
        OUTCOME_TEST_READS.fetch_add(1, Ordering::Relaxed);
        if self.close_on_read {
            context.local_pipeline_context().close();
        } else {
            if self.close_on_ready || self.close_on_feedback {
                self.lifecycle_context = Some(context.local_pipeline_context());
            }
            if self.feedback_write.is_some() {
                self.feedback_context = Some(context.context_handle());
            }
            self.activation_armed = self.activation_write.is_some();
        }
        let mut outcome = TailReadOutcome::new(self.result);
        if let Some(write) = self.read_write.take() {
            outcome = outcome.with_write(write);
        }
        if let Some(token) = self.read_feedback.take() {
            outcome = outcome.with_feedback(token);
        }
        self.leave_callback();
        outcome
    }

    fn on_write_ready_outcome(&mut self) -> TailWriteOutcome {
        self.enter_callback();
        const READY_INDEX: usize = 0;
        const SECOND_READY_INDEX: usize = 1;
        let ready_count = OUTCOME_TEST_WRITE_READY.fetch_add(1, Ordering::Relaxed);
        match ready_count {
            0 => record_outcome_order(&OUTCOME_TEST_READY_ORDERS[READY_INDEX]),
            1 => record_outcome_order(&OUTCOME_TEST_READY_ORDERS[SECOND_READY_INDEX]),
            _ => {}
        }
        let outcome = self
            .ready_outcomes
            .pop_front()
            .unwrap_or_else(|| TailWriteOutcome::new(None));
        if self.close_on_ready
            && let Some(context) = self.lifecycle_context.take()
        {
            context.close();
        }
        self.leave_callback();
        outcome
    }

    fn on_write_result(&mut self, token: TailWriteFeedbackToken, result: HandlerResult) {
        self.enter_callback();
        match result {
            HandlerResult::Success => {
                OUTCOME_TEST_FEEDBACK_SUCCESS.fetch_add(1, Ordering::Relaxed);
            }
            HandlerResult::Backpressure => {
                OUTCOME_TEST_FEEDBACK_BACKPRESSURE.fetch_add(1, Ordering::Relaxed);
            }
            HandlerResult::Error => {
                OUTCOME_TEST_FEEDBACK_ERROR.fetch_add(1, Ordering::Relaxed);
            }
        }
        let feedback_index = OUTCOME_TEST_FEEDBACK_COUNT.fetch_add(1, Ordering::Relaxed) as usize;
        if let Some(order) = OUTCOME_TEST_FEEDBACK_ORDERS.get(feedback_index) {
            record_outcome_order(order);
        }
        OUTCOME_TEST_LAST_TOKEN.store(token.get(), Ordering::Relaxed);
        if let (Some(context), Some(write)) =
            (self.feedback_context.take(), self.feedback_write.take())
        {
            context.fire_write(write);
        }
        if let Some(context) = self.lifecycle_context.take() {
            context.close();
        }
        self.leave_callback();
    }

    fn on_pipeline_active_write(&mut self) -> Option<TailWrite> {
        self.enter_callback();
        let write = if self.activation_armed {
            OUTCOME_TEST_ACTIVE.fetch_add(1, Ordering::Relaxed);
            record_outcome_order(&OUTCOME_TEST_ACTIVE_ORDER);
            self.activation_write.take()
        } else {
            None
        };
        self.leave_callback();
        write
    }

    fn on_exception(&mut self) {
        self.enter_callback();
        OUTCOME_TEST_EXCEPTIONS.fetch_add(1, Ordering::Relaxed);
        record_outcome_order(&OUTCOME_TEST_EXCEPTION_ORDER);
        self.leave_callback();
    }

    fn on_pipeline_inactive(&mut self) {
        self.enter_callback();
        OUTCOME_TEST_INACTIVE.fetch_add(1, Ordering::Relaxed);
        record_outcome_order(&OUTCOME_TEST_INACTIVE_ORDER);
        self.leave_callback();
    }

    fn handler_removed(&mut self) {
        self.enter_callback();
        OUTCOME_TEST_REMOVED.fetch_add(1, Ordering::Relaxed);
        record_outcome_order(&OUTCOME_TEST_REMOVED_ORDER);
        self.leave_callback();
    }
}

impl RustTailEndpoint for ReturnedWriteBenchTail {
    fn on_read(
        &mut self,
        _context: &mut CallbackContext<'_>,
        _message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        HandlerResult::Error
    }

    fn on_read_outcome(
        &mut self,
        _context: &mut CallbackContext<'_>,
        mut message: RustTypeErasedBox<'_>,
    ) -> TailReadOutcome {
        let write = TailWrite::try_new(message.take::<BytesPtr>())
            .expect("pipeline BytesPtr should be non-null");
        TailReadOutcome::new(HandlerResult::Success)
            .with_write(write)
            .with_feedback(TailWriteFeedbackToken::try_new(1).expect("nonzero benchmark token"))
    }
}

impl RustTailEndpoint for LegacyWriteBenchTail {
    fn on_read(
        &mut self,
        context: &mut CallbackContext<'_>,
        mut message: RustTypeErasedBox<'_>,
    ) -> HandlerResult {
        context.fire_write(message.take::<BytesPtr>())
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

pub(crate) fn reset_lifecycle_write_test_counts() {
    for counter in [
        &WRITE_TEST_EXCEPTIONS,
        &WRITE_TEST_INACTIVE,
        &WRITE_TEST_REMOVED,
        &WRITE_TEST_REJECTED_NULL,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
}

pub(crate) fn lifecycle_write_test_counts() -> [u32; 4] {
    [
        WRITE_TEST_EXCEPTIONS.load(Ordering::Relaxed),
        WRITE_TEST_INACTIVE.load(Ordering::Relaxed),
        WRITE_TEST_REMOVED.load(Ordering::Relaxed),
        WRITE_TEST_REJECTED_NULL.load(Ordering::Relaxed),
    ]
}

pub(crate) fn reset_read_outcome_test_counts() {
    for counter in [
        &OUTCOME_TEST_READS,
        &OUTCOME_TEST_FEEDBACK_SUCCESS,
        &OUTCOME_TEST_FEEDBACK_BACKPRESSURE,
        &OUTCOME_TEST_FEEDBACK_ERROR,
        &OUTCOME_TEST_WRITE_READY,
        &OUTCOME_TEST_EXCEPTIONS,
        &OUTCOME_TEST_INACTIVE,
        &OUTCOME_TEST_REMOVED,
        &OUTCOME_TEST_ACTIVE,
        &OUTCOME_TEST_REENTRANCY,
        &OUTCOME_TEST_SEQUENCE,
        &OUTCOME_TEST_FEEDBACK_COUNT,
        &OUTCOME_TEST_EXCEPTION_ORDER,
        &OUTCOME_TEST_INACTIVE_ORDER,
        &OUTCOME_TEST_REMOVED_ORDER,
        &OUTCOME_TEST_ACTIVE_ORDER,
    ] {
        counter.store(0, Ordering::Relaxed);
    }
    OUTCOME_TEST_FEEDBACK_ORDERS
        .iter()
        .chain(OUTCOME_TEST_READY_ORDERS.iter())
        .for_each(|counter| counter.store(0, Ordering::Relaxed));
    OUTCOME_TEST_LAST_TOKEN.store(0, Ordering::Relaxed);
}

pub(crate) fn read_outcome_test_counts() -> [u64; 20] {
    [
        OUTCOME_TEST_READS.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_FEEDBACK_SUCCESS.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_FEEDBACK_BACKPRESSURE
            .load(Ordering::Relaxed)
            .into(),
        OUTCOME_TEST_FEEDBACK_ERROR.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_WRITE_READY.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_EXCEPTIONS.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_INACTIVE.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_REMOVED.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_REENTRANCY.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_LAST_TOKEN.load(Ordering::Relaxed),
        OUTCOME_TEST_FEEDBACK_ORDERS[0]
            .load(Ordering::Relaxed)
            .into(),
        OUTCOME_TEST_READY_ORDERS[0].load(Ordering::Relaxed).into(),
        OUTCOME_TEST_FEEDBACK_ORDERS[1]
            .load(Ordering::Relaxed)
            .into(),
        OUTCOME_TEST_READY_ORDERS[1].load(Ordering::Relaxed).into(),
        OUTCOME_TEST_FEEDBACK_ORDERS[2]
            .load(Ordering::Relaxed)
            .into(),
        OUTCOME_TEST_EXCEPTION_ORDER.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_INACTIVE_ORDER.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_REMOVED_ORDER.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_ACTIVE.load(Ordering::Relaxed).into(),
        OUTCOME_TEST_ACTIVE_ORDER.load(Ordering::Relaxed).into(),
    ]
}
