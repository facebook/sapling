/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is dual-licensed under either the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree or the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree. You may select, at your option, one of the
 * above-listed licenses.
 */

#[cfg(unix)]
pub use tokio::net::UnixListener;
#[cfg(unix)]
pub use tokio::net::UnixStream;
#[cfg(unix)]
pub use tokio::net::unix::OwnedReadHalf;
#[cfg(unix)]
pub use tokio::net::unix::OwnedWriteHalf;

#[cfg(windows)]
mod windows {
    use std::future::Future;
    use std::io;
    use std::mem;
    use std::net::Shutdown;
    use std::os::windows::io::AsRawSocket;
    use std::os::windows::io::AsSocket;
    use std::os::windows::io::BorrowedSocket;
    use std::os::windows::io::RawSocket;
    use std::path::Path;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::MutexGuard;
    use std::sync::Weak;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;
    use std::time::Duration;

    /// Compat layer for providing UNIX domain socket on Windows
    use async_io::Async;
    use async_io::Timer;
    use tokio::io::AsyncRead;
    use tokio::io::AsyncWrite;
    use tokio::io::ReadBuf;

    /// Wrapper for uds_windows::UnixListener that implements AsSocket.
    /// This is needed because async-io 2.x requires AsSocket for Async::new().
    #[derive(Debug)]
    struct UnixListenerWrapper(uds_windows::UnixListener);

    impl AsSocket for UnixListenerWrapper {
        fn as_socket(&self) -> BorrowedSocket<'_> {
            // SAFETY: The raw socket is valid for the lifetime of self
            unsafe { BorrowedSocket::borrow_raw(self.0.as_raw_socket()) }
        }
    }

    impl AsRawSocket for UnixListenerWrapper {
        fn as_raw_socket(&self) -> RawSocket {
            self.0.as_raw_socket()
        }
    }

    impl UnixListenerWrapper {
        fn accept(&self) -> io::Result<(uds_windows::UnixStream, uds_windows::SocketAddr)> {
            self.0.accept()
        }
    }

    /// Helper function to prevent vtable mismatches in optimized builds by cloning
    /// the waker at the async-io runtime boundary. This ensures consistent waker
    /// identity across cross-runtime calls.
    fn with_cloned_waker<T>(cx: &Context<'_>, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
        let cloned_waker = cx.waker().clone();
        let mut preserving_cx = Context::from_waker(&cloned_waker);
        f(&mut preserving_cx)
    }

    enum AcceptPoll<T> {
        Ready(io::Result<T>),
        Pending,
        Backoff,
    }

    struct ArmedAcceptTimer {
        timer: Arc<Mutex<Timer>>,
        initial_poll: AcceptTimerPoll,
    }

    enum AcceptTimerArm {
        Armed(ArmedAcceptTimer),
        WakeOwnedElsewhere,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum AcceptTimerPoll {
        Pending,
        Elapsed,
        Invalidated,
    }

    enum AcceptBackoffPoll<T> {
        Ready(io::Result<T>),
        ReadinessPending,
        WakePending,
    }

    fn finish_accept_backoff<T>(
        cx: &mut Context<'_>,
        mut accept: impl FnMut() -> io::Result<T>,
        mut poll_readable: impl FnMut(&mut Context<'_>) -> Poll<io::Result<()>>,
        mut ensure_timer_wake: impl FnMut(&mut Context<'_>),
    ) -> AcceptBackoffPoll<T> {
        match accept() {
            Ok(accepted) => AcceptBackoffPoll::Ready(Ok(accepted)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                match poll_readable(cx) {
                    Poll::Ready(Err(error)) => return AcceptBackoffPoll::Ready(Err(error)),
                    Poll::Pending => return AcceptBackoffPoll::ReadinessPending,
                    Poll::Ready(Ok(())) => {}
                }

                // This either leaves a live timer registered or observes that
                // another operation already owns the next wake.
                ensure_timer_wake(cx);
                // A live timer deliberately bounds retries when the source
                // cannot return `Pending`. A connection arriving after the
                // final accept can therefore wait for the remaining backoff,
                // capped at 100 ms, instead of causing an unbounded self-wake
                // loop.
                AcceptBackoffPoll::WakePending
            }
            Err(error) => AcceptBackoffPoll::Ready(Err(error)),
        }
    }

    fn poll_accept_raw<T>(
        cx: &mut Context<'_>,
        mut accept: impl FnMut() -> io::Result<T>,
        poll_readable: impl FnMut(&mut Context<'_>) -> Poll<io::Result<()>>,
    ) -> AcceptPoll<T> {
        // Accept before consulting readiness: a readable listener is only a
        // hint (a peer that connected and dropped can leave nothing to take),
        // while several queued connections can share one readiness edge.
        match accept() {
            Ok(accepted) => AcceptPoll::Ready(Ok(accepted)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                poll_accept_after_would_block(cx, accept, poll_readable)
            }
            // Match Tokio listener semantics by surfacing every other per-call
            // error. Callers choose whether to retry; acmux's accept loop does
            // so with a new future.
            Err(error) => AcceptPoll::Ready(Err(error)),
        }
    }

    fn poll_accept_after_would_block<T>(
        cx: &mut Context<'_>,
        mut accept: impl FnMut() -> io::Result<T>,
        mut poll_readable: impl FnMut(&mut Context<'_>) -> Poll<io::Result<()>>,
    ) -> AcceptPoll<T> {
        match poll_readable(cx) {
            Poll::Pending => return AcceptPoll::Pending,
            Poll::Ready(Err(error)) => return AcceptPoll::Ready(Err(error)),
            Poll::Ready(Ok(())) => {}
        }

        match accept() {
            Ok(accepted) => AcceptPoll::Ready(Ok(accepted)),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                // `async_io::Async::poll_readable` clears its readiness tick
                // before returning `Ready`. Poll it again so this task normally
                // registers its waker before parking. A second `Ready` means a
                // distinct event raced with registration and selects the
                // bounded fallback instead.
                match poll_readable(cx) {
                    Poll::Pending => AcceptPoll::Pending,
                    Poll::Ready(Err(error)) => AcceptPoll::Ready(Err(error)),
                    Poll::Ready(Ok(())) => AcceptPoll::Backoff,
                }
            }
            Err(error) => AcceptPoll::Ready(Err(error)),
        }
    }

    pub struct OwnedReadHalf {
        inner: Arc<UnixStream>,
    }

    impl OwnedReadHalf {
        fn new(inner: Arc<UnixStream>) -> Self {
            Self { inner }
        }
    }

    impl AsyncRead for OwnedReadHalf {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            self.inner.poll_read_priv(cx, buf)
        }
    }

    pub struct OwnedWriteHalf {
        inner: Arc<UnixStream>,
        shutdown_on_drop: bool,
    }

    impl OwnedWriteHalf {
        fn new(inner: Arc<UnixStream>) -> Self {
            Self {
                inner,
                shutdown_on_drop: true,
            }
        }
    }

    impl Drop for OwnedWriteHalf {
        fn drop(&mut self) {
            if self.shutdown_on_drop {
                let _ = self.inner.async_ref().as_ref().shutdown(Shutdown::Write);
            }
        }
    }

    impl AsyncWrite for OwnedWriteHalf {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_write(
                    Pin::new(&mut self.inner.async_ref()),
                    preserving_cx,
                    buf,
                )
            })
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_flush(
                    Pin::new(&mut self.inner.async_ref()),
                    preserving_cx,
                )
            })
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_close(
                    Pin::new(&mut self.inner.async_ref()),
                    preserving_cx,
                )
            })
        }
    }

    #[derive(Debug)]
    pub struct UnixStream(Async<uds_windows::UnixStream>);

    impl UnixStream {
        pub async fn connect<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let stream = uds_windows::UnixStream::connect(path)?;
            Self::from_std(stream)
        }

        fn from_std(stream: uds_windows::UnixStream) -> io::Result<Self> {
            let stream = Async::new(stream)?;

            Ok(UnixStream(stream))
        }

        fn async_ref(&self) -> &Async<uds_windows::UnixStream> {
            &self.0
        }

        pub fn into_split(self) -> (OwnedReadHalf, OwnedWriteHalf) {
            let this = Arc::new(self);
            (OwnedReadHalf::new(this.clone()), OwnedWriteHalf::new(this))
        }

        fn poll_read_priv(
            &self,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                let result = futures::AsyncRead::poll_read(
                    Pin::new(&mut self.async_ref()),
                    preserving_cx,
                    buf.initialize_unfilled(),
                );

                match result {
                    Poll::Ready(Ok(written)) => {
                        tracing::trace!(?written, "UnixStream::poll_read");
                        buf.set_filled(written);
                        Poll::Ready(Ok(()))
                    }
                    Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                    Poll::Pending => Poll::Pending,
                }
            })
        }
    }

    impl AsyncRead for UnixStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<Result<(), io::Error>> {
            self.poll_read_priv(cx, buf)
        }
    }

    impl AsyncWrite for UnixStream {
        fn poll_write(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<Result<usize, io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_write(Pin::new(&mut self.async_ref()), preserving_cx, buf)
            })
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_flush(Pin::new(&mut self.async_ref()), preserving_cx)
            })
        }

        fn poll_shutdown(
            self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), io::Error>> {
            with_cloned_waker(cx, |preserving_cx| {
                futures::AsyncWrite::poll_close(Pin::new(&mut self.async_ref()), preserving_cx)
            })
        }
    }

    const ACCEPT_REARM_BACKOFF_INITIAL: Duration = Duration::from_millis(1);
    const ACCEPT_REARM_BACKOFF_MAX: Duration = Duration::from_millis(100);

    fn next_accept_backoff(current: Duration, maximum: Duration) -> Duration {
        current.saturating_mul(2).min(maximum)
    }

    #[derive(Debug)]
    enum AcceptTimerPollState {
        Idle,
        Polling,
        RepollRequested,
    }

    #[derive(Debug)]
    struct AcceptTimerSlot {
        timer: Arc<Mutex<Timer>>,
        poll_state: AcceptTimerPollState,
    }

    #[derive(Debug)]
    enum AcceptTimerState {
        Unarmed,
        Creating { generation: u64 },
        Armed(AcceptTimerSlot),
    }

    impl AcceptTimerState {
        fn armed(&self) -> Option<&AcceptTimerSlot> {
            match self {
                Self::Armed(slot) => Some(slot),
                Self::Unarmed | Self::Creating { .. } => None,
            }
        }

        fn armed_mut(&mut self) -> Option<&mut AcceptTimerSlot> {
            match self {
                Self::Armed(slot) => Some(slot),
                Self::Unarmed | Self::Creating { .. } => None,
            }
        }

        fn is_armed(&self) -> bool {
            matches!(self, Self::Armed(_))
        }

        fn is_unarmed(&self) -> bool {
            matches!(self, Self::Unarmed)
        }

        fn take(&mut self) -> Self {
            mem::replace(self, Self::Unarmed)
        }
    }

    #[derive(Debug)]
    struct AcceptBackoffState {
        timer: AcceptTimerState,
        waiter: Option<Waker>,
        next_delay: Duration,
        generation: u64,
    }

    #[derive(Debug)]
    struct AcceptBackoff {
        // Unlike `accept()`, `poll_accept` has no caller-owned future to retain.
        // A shared delayed wake avoids hot-spinning on stale readiness while
        // preserving its most-recent-waker contract. Per-call timers would
        // wake displaced callers, and removing the method would break the API.
        state: Mutex<AcceptBackoffState>,
        initial_delay: Duration,
        maximum_delay: Duration,
        wake: Arc<AcceptBackoffWake>,
    }

    struct AcceptTimerCreationGuard<'a> {
        backoff: &'a AcceptBackoff,
        generation: u64,
        active: bool,
    }

    impl AcceptTimerCreationGuard<'_> {
        fn complete(mut self) {
            self.active = false;
        }
    }

    impl Drop for AcceptTimerCreationGuard<'_> {
        fn drop(&mut self) {
            if self.active {
                self.backoff.cancel_timer_creation(self.generation);
            }
        }
    }

    struct AcceptTimerPollGuard<'a> {
        backoff: &'a AcceptBackoff,
        timer: Arc<Mutex<Timer>>,
        active: bool,
    }

    impl AcceptTimerPollGuard<'_> {
        fn finish(mut self, cx: &mut Context<'_>, result: Poll<()>) -> AcceptTimerPoll {
            let result = self.backoff.finish_timer_poll(cx, &self.timer, result);
            self.active = false;
            result
        }
    }

    impl Drop for AcceptTimerPollGuard<'_> {
        fn drop(&mut self) {
            if self.active {
                self.backoff.cancel_timer_poll(&self.timer);
            }
        }
    }

    impl AcceptBackoff {
        fn new() -> Arc<Self> {
            Self::with_delays(ACCEPT_REARM_BACKOFF_INITIAL, ACCEPT_REARM_BACKOFF_MAX)
        }

        fn with_delays(initial_delay: Duration, maximum_delay: Duration) -> Arc<Self> {
            Arc::new_cyclic(|owner| Self {
                state: Mutex::new(AcceptBackoffState {
                    timer: AcceptTimerState::Unarmed,
                    waiter: None,
                    next_delay: initial_delay,
                    generation: 0,
                }),
                initial_delay,
                maximum_delay,
                wake: Arc::new(AcceptBackoffWake(owner.clone())),
            })
        }

        fn arm_and_poll(&self, cx: &mut Context<'_>) -> AcceptTimerArm {
            self.register(cx.waker());
            self.poll_or_create_timer(cx)
        }

        fn poll_or_create_timer(&self, cx: &mut Context<'_>) -> AcceptTimerArm {
            let Some(timer) = self.get_or_create_timer() else {
                return AcceptTimerArm::WakeOwnedElsewhere;
            };
            let initial_poll = self.poll_timer(cx, timer.clone());
            AcceptTimerArm::Armed(ArmedAcceptTimer {
                timer,
                initial_poll,
            })
        }

        fn get_or_create_timer(&self) -> Option<Arc<Mutex<Timer>>> {
            self.get_or_create_timer_with(|delay| Arc::new(Mutex::new(Timer::after(delay))))
        }

        fn get_or_create_timer_with(
            &self,
            create: impl FnOnce(Duration) -> Arc<Mutex<Timer>>,
        ) -> Option<Arc<Mutex<Timer>>> {
            let (generation, delay) = {
                let mut state = self.lock_state();
                if let Some(slot) = state.timer.armed() {
                    return Some(slot.timer.clone());
                }
                if matches!(&state.timer, AcceptTimerState::Creating { .. }) {
                    return None;
                }

                let generation = state.generation;
                let delay = state.next_delay;
                state.timer = AcceptTimerState::Creating { generation };
                (generation, delay)
            };
            let reservation = AcceptTimerCreationGuard {
                backoff: self,
                generation,
                active: true,
            };

            // Reserve before touching the reactor so contending pollers never
            // create throwaway timers. Construction stays outside the state
            // lock; only a genuine concurrent reset can invalidate this one
            // bounded candidate before installation.
            let candidate = create(delay);
            let selected = self.install_timer_candidate(generation, delay, candidate);
            reservation.complete();
            selected
        }

        fn install_timer_candidate(
            &self,
            generation: u64,
            delay: Duration,
            candidate: Arc<Mutex<Timer>>,
        ) -> Option<Arc<Mutex<Timer>>> {
            {
                let mut state = self.lock_state();
                if let Some(slot) = state.timer.armed() {
                    return Some(slot.timer.clone());
                }
                let owns_reservation = matches!(
                    &state.timer,
                    AcceptTimerState::Creating {
                        generation: reserved
                    } if *reserved == generation
                ) && state.generation == generation;
                if !owns_reservation {
                    return None;
                }

                state.next_delay = next_accept_backoff(delay, self.maximum_delay);
                state.generation = state.generation.wrapping_add(1);
                state.timer = AcceptTimerState::Armed(AcceptTimerSlot {
                    timer: candidate.clone(),
                    poll_state: AcceptTimerPollState::Idle,
                });
            }
            Some(candidate)
        }

        fn cancel_timer_creation(&self, generation: u64) {
            let waiter = {
                let mut state = self.lock_state();
                if !matches!(
                    &state.timer,
                    AcceptTimerState::Creating {
                        generation: reserved
                    } if *reserved == generation
                ) {
                    return;
                }

                state.timer = AcceptTimerState::Unarmed;
                state.generation = state.generation.wrapping_add(1);
                state.waiter.take()
            };
            Self::wake_waiter(waiter);
        }

        fn ensure_backoff_wake(&self, cx: &mut Context<'_>) {
            if let AcceptTimerArm::Armed(ArmedAcceptTimer {
                initial_poll: AcceptTimerPoll::Elapsed,
                ..
            }) = self.poll_or_create_timer(cx)
            {
                self.wake_latest();
            }
        }

        fn poll_timer(&self, cx: &mut Context<'_>, timer: Arc<Mutex<Timer>>) -> AcceptTimerPoll {
            self.poll_timer_with(cx, timer, |timer, timer_cx| {
                Pin::new(timer).poll(timer_cx).map(|_| ())
            })
        }

        fn poll_timer_with(
            &self,
            cx: &mut Context<'_>,
            timer: Arc<Mutex<Timer>>,
            poll: impl FnOnce(&mut Timer, &mut Context<'_>) -> Poll<()>,
        ) -> AcceptTimerPoll {
            // The timer and readiness source see one stable bridge waker, so
            // concurrent direct polls cannot trigger async-io's conflicting-
            // waker ping-pong. The bridge forwards to the most recent caller,
            // matching Tokio's `poll_accept` contract.
            {
                let mut state = self.lock_state();
                let Some(slot) = state.timer.armed_mut() else {
                    return AcceptTimerPoll::Invalidated;
                };
                if !Arc::ptr_eq(&slot.timer, &timer) {
                    return AcceptTimerPoll::Invalidated;
                }
                match slot.poll_state {
                    AcceptTimerPollState::Idle => {
                        slot.poll_state = AcceptTimerPollState::Polling;
                    }
                    AcceptTimerPollState::Polling | AcceptTimerPollState::RepollRequested => {
                        slot.poll_state = AcceptTimerPollState::RepollRequested;
                        return AcceptTimerPoll::Pending;
                    }
                }
            }

            let poll_guard = AcceptTimerPollGuard {
                backoff: self,
                timer: timer.clone(),
                active: true,
            };
            let broadcast_waker = Waker::from(self.wake.clone());
            let mut timer_cx = Context::from_waker(&broadcast_waker);
            let mut timer_guard = match timer.lock() {
                Ok(timer) => timer,
                Err(poisoned) => poisoned.into_inner(),
            };
            let timer_result = poll(&mut timer_guard, &mut timer_cx);
            drop(timer_guard);

            poll_guard.finish(cx, timer_result)
        }

        fn finish_timer_poll(
            &self,
            cx: &mut Context<'_>,
            timer: &Arc<Mutex<Timer>>,
            timer_result: Poll<()>,
        ) -> AcceptTimerPoll {
            let (timer_result, displaced_waiter, detached_timer) = {
                let mut state = self.lock_state();
                if !state
                    .timer
                    .armed()
                    .is_some_and(|slot| Arc::ptr_eq(&slot.timer, &timer))
                {
                    return AcceptTimerPoll::Invalidated;
                }

                if timer_result.is_ready() {
                    let detached_timer = state.timer.take();
                    state.generation = state.generation.wrapping_add(1);
                    let displaced_waiter = if state
                        .waiter
                        .as_ref()
                        .is_some_and(|waiter| waiter.will_wake(cx.waker()))
                    {
                        // `will_wake` is only a redundant-wake optimization:
                        // a false negative takes and wakes this waiter below.
                        // If final readiness is also immediately ready, the
                        // consumed timer must still self-wake this caller.
                        None
                    } else {
                        state.waiter.take()
                    };
                    (
                        AcceptTimerPoll::Elapsed,
                        displaced_waiter,
                        Some(detached_timer),
                    )
                } else {
                    let repoll_requested = state.timer.armed_mut().is_some_and(|slot| {
                        let repoll_requested =
                            matches!(slot.poll_state, AcceptTimerPollState::RepollRequested);
                        slot.poll_state = AcceptTimerPollState::Idle;
                        repoll_requested
                    });
                    (
                        AcceptTimerPoll::Pending,
                        repoll_requested.then(|| state.waiter.take()).flatten(),
                        None,
                    )
                }
            };
            drop(detached_timer);
            Self::wake_waiter(displaced_waiter);
            timer_result
        }

        fn cancel_timer_poll(&self, timer: &Arc<Mutex<Timer>>) {
            let displaced_waiter = {
                let mut state = self.lock_state();
                let owned_poll = state.timer.armed_mut().is_some_and(|slot| {
                    if !Arc::ptr_eq(&slot.timer, timer) {
                        return false;
                    }
                    slot.poll_state = AcceptTimerPollState::Idle;
                    true
                });
                owned_poll.then(|| state.waiter.take()).flatten()
            };
            // A waiter may have observed `Creating` and returned before this
            // timer was installed, so `RepollRequested` is not the only proof
            // of contention. On unwind, always wake the latest registered
            // caller after restoring the slot to `Idle`.
            Self::wake_waiter(displaced_waiter);
        }

        fn readiness_rearmed(&self) {
            let (timer, waiter_to_wake) = {
                let mut state = self.lock_state();
                state.next_delay = self.initial_delay;
                state.generation = state.generation.wrapping_add(1);
                let timer = state.timer.take();
                let waiter_to_wake = if !timer.is_unarmed() {
                    state.waiter.take()
                } else {
                    None
                };
                (timer, waiter_to_wake)
            };
            drop(timer);
            // Any direct poll may rely on the shared timer this caller
            // invalidated, even when both calls use the same waker. Schedule
            // the latest caller to register again with the readiness source.
            Self::wake_waiter(waiter_to_wake);
        }

        fn reset(&self, current_waker: Option<&Waker>) {
            let (timer, waiter) = {
                let mut state = self.lock_state();
                state.next_delay = self.initial_delay;
                state.generation = state.generation.wrapping_add(1);
                (state.timer.take(), state.waiter.take())
            };
            drop(timer);
            // Completion invalidates the shared timer. Wake whichever direct
            // poller is currently registered so it can recompute from an
            // accept-first state, unless that poller is completing now.
            match current_waker {
                Some(current) => Self::wake_waiter_unless(waiter, current),
                None => Self::wake_waiter(waiter),
            }
        }

        fn register(&self, waker: &Waker) {
            let replacement = waker.clone();
            let displaced = {
                let mut state = self.lock_state();
                state.waiter.replace(replacement)
            };
            drop(displaced);
        }

        fn with_broadcast_waker<T>(&self, f: impl FnOnce(&mut Context<'_>) -> T) -> T {
            let broadcast_waker = Waker::from(self.wake.clone());
            let mut broadcast_cx = Context::from_waker(&broadcast_waker);
            f(&mut broadcast_cx)
        }

        fn lock_state(&self) -> MutexGuard<'_, AcceptBackoffState> {
            match self.state.lock() {
                Ok(state) => state,
                Err(poisoned) => poisoned.into_inner(),
            }
        }

        fn wake_waiter(waiter: Option<Waker>) {
            if let Some(waiter) = waiter {
                waiter.wake();
            }
        }

        fn wake_waiter_unless(waiter: Option<Waker>, current: &Waker) {
            if let Some(waiter) = waiter
                && !waiter.will_wake(current)
            {
                // `will_wake` may return a false negative; that only schedules
                // one redundant poll after completion.
                waiter.wake();
            }
        }

        fn wake_latest(&self) {
            let waiter = {
                let mut state = self.lock_state();
                state.waiter.take()
            };
            Self::wake_waiter(waiter);
        }
    }

    #[derive(Debug)]
    struct AcceptBackoffWake(Weak<AcceptBackoff>);

    impl Wake for AcceptBackoffWake {
        fn wake(self: Arc<Self>) {
            if let Some(backoff) = self.0.upgrade() {
                backoff.wake_latest();
            }
        }

        fn wake_by_ref(self: &Arc<Self>) {
            if let Some(backoff) = self.0.upgrade() {
                backoff.wake_latest();
            }
        }
    }

    #[derive(Debug)]
    pub struct UnixListener {
        inner: Async<UnixListenerWrapper>,
        accept_backoff: Arc<AcceptBackoff>,
    }

    impl UnixListener {
        pub fn bind<P: AsRef<Path>>(path: P) -> io::Result<Self> {
            let listener = uds_windows::UnixListener::bind(path)?;
            let wrapper = UnixListenerWrapper(listener);
            let inner = Async::new(wrapper)?;

            Ok(UnixListener {
                inner,
                accept_backoff: AcceptBackoff::new(),
            })
        }

        /// Accepts one connection with an independent readiness registration.
        ///
        /// Concurrent futures returned by this method are independently
        /// wakeable, and dropping one does not cancel another's registration.
        pub async fn accept(&self) -> io::Result<(UnixStream, uds_windows::SocketAddr)> {
            let accept = self.inner.read_with(UnixListenerWrapper::accept);
            futures::pin_mut!(accept);
            let result = futures::future::poll_fn(|cx| {
                with_cloned_waker(cx, |preserving_cx| accept.as_mut().poll(preserving_cx))
            })
            .await;
            let (stream, addr) = match result {
                Ok(accepted) => accepted,
                // An async error made no listener progress and owns no direct-
                // poll state, so it must not clear a concurrent direct poller's
                // timer or waker.
                Err(error) => return Err(error),
            };
            // Accepting a connection is global listener progress. Invalidate
            // stale direct-poll backoff and wake its latest registered caller.
            self.accept_backoff.reset(None);
            UnixStream::from_std(stream).map(|stream| (stream, addr))
        }

        /// Polls for an accepted connection.
        ///
        /// As with Tokio's listener, when multiple tasks call this method only
        /// the waker from the most recent call is scheduled. Use [`Self::accept`]
        /// for independently registered concurrent accept futures.
        pub fn poll_accept(
            &self,
            cx: &mut Context<'_>,
        ) -> Poll<io::Result<(UnixStream, uds_windows::SocketAddr)>> {
            with_cloned_waker(cx, |preserving_cx| {
                let result = poll_accept_raw(
                    preserving_cx,
                    || self.inner.get_ref().accept(),
                    |readiness_cx| self.poll_accept_readable(readiness_cx),
                );
                match result {
                    AcceptPoll::Ready(result) => self.finish_direct_accept(preserving_cx, result),
                    AcceptPoll::Pending => {
                        // Readiness now owns the next wake, so any fallback
                        // timer is stale and a future isolated backoff starts
                        // again at the initial delay.
                        self.accept_backoff.readiness_rearmed();
                        Poll::Pending
                    }
                    AcceptPoll::Backoff => {
                        // The second readiness edge arrived after the previous
                        // accept attempt, so try accept once more before the
                        // final readiness registration decides how to park. A
                        // timer is created only if that registration also sees
                        // immediate readiness, so it cannot be invalidated by
                        // this call's own final readiness poll.
                        match finish_accept_backoff(
                            preserving_cx,
                            || self.inner.get_ref().accept(),
                            |readiness_cx| self.poll_accept_readable(readiness_cx),
                            |timer_cx| self.accept_backoff.ensure_backoff_wake(timer_cx),
                        ) {
                            AcceptBackoffPoll::Ready(result) => {
                                self.finish_direct_accept(preserving_cx, result)
                            }
                            AcceptBackoffPoll::ReadinessPending => {
                                self.accept_backoff.readiness_rearmed();
                                Poll::Pending
                            }
                            AcceptBackoffPoll::WakePending => Poll::Pending,
                        }
                    }
                }
            })
        }

        fn poll_accept_readable(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            self.accept_backoff.register(cx.waker());
            self.accept_backoff
                .with_broadcast_waker(|readiness_cx| self.inner.poll_readable(readiness_cx))
        }

        fn finish_direct_accept(
            &self,
            cx: &Context<'_>,
            result: io::Result<(uds_windows::UnixStream, uds_windows::SocketAddr)>,
        ) -> Poll<io::Result<(UnixStream, uds_windows::SocketAddr)>> {
            self.accept_backoff.reset(Some(cx.waker()));
            Poll::Ready(result.and_then(|(stream, addr)| {
                UnixStream::from_std(stream).map(|stream| (stream, addr))
            }))
        }
    }

    #[cfg(test)]
    mod tests {
        use std::future::Future;
        use std::io;
        use std::io::ErrorKind;
        use std::panic;
        use std::panic::AssertUnwindSafe;
        use std::path::PathBuf;
        use std::sync::Arc;
        use std::sync::Mutex;
        use std::sync::Weak;
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::AtomicU64;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        use std::sync::mpsc;
        use std::task::Context;
        use std::task::Poll;
        use std::task::RawWaker;
        use std::task::RawWakerVTable;
        use std::task::Wake;
        use std::task::Waker;
        use std::time::Duration;

        use async_io::Timer;
        use futures::future::Either;
        use futures::task::noop_waker;

        use super::AcceptBackoff;
        use super::AcceptBackoffPoll;
        use super::AcceptPoll;
        use super::AcceptTimerArm;
        use super::AcceptTimerPoll;
        use super::AcceptTimerPollState;
        use super::ArmedAcceptTimer;
        use super::UnixListener;
        use super::UnixStream;
        use super::finish_accept_backoff;
        use super::next_accept_backoff;
        use super::poll_accept_raw;

        const QUEUED_CLIENTS: usize = 8;
        const BACKLOG_DRAIN_DEADLINE: Duration = Duration::from_secs(30);

        static NEXT_SOCKET_ID: AtomicU64 = AtomicU64::new(0);

        fn expect_armed_timer(arm: AcceptTimerArm) -> ArmedAcceptTimer {
            match arm {
                AcceptTimerArm::Armed(timer) => timer,
                AcceptTimerArm::WakeOwnedElsewhere => {
                    panic!("timer wake should not be owned elsewhere")
                }
            }
        }

        struct SocketCleanup(PathBuf);

        impl Drop for SocketCleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }

        struct WakeCounter(AtomicUsize);

        impl Wake for WakeCounter {
            fn wake(self: Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }

            fn wake_by_ref(self: &Arc<Self>) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }

        struct WakeSender(mpsc::Sender<()>);

        impl Wake for WakeSender {
            fn wake(self: Arc<Self>) {
                let _ = self.0.send(());
            }

            fn wake_by_ref(self: &Arc<Self>) {
                let _ = self.0.send(());
            }
        }

        struct NonIdentityWakerData(Arc<AtomicUsize>);

        fn non_identity_raw_waker(wake_count: Arc<AtomicUsize>) -> RawWaker {
            let data = Box::into_raw(Box::new(NonIdentityWakerData(wake_count)));
            RawWaker::new(data.cast(), &NON_IDENTITY_WAKER_VTABLE)
        }

        unsafe fn clone_non_identity_waker(data: *const ()) -> RawWaker {
            // SAFETY: Every data pointer in this vtable comes from a live
            // `Box<NonIdentityWakerData>` created by `non_identity_raw_waker`.
            let data = unsafe { &*data.cast::<NonIdentityWakerData>() };
            non_identity_raw_waker(data.0.clone())
        }

        unsafe fn wake_non_identity_waker(data: *const ()) {
            // SAFETY: `wake` consumes exactly one boxed data pointer.
            let data = unsafe { Box::from_raw(data.cast_mut().cast::<NonIdentityWakerData>()) };
            data.0.fetch_add(1, Ordering::Relaxed);
        }

        unsafe fn wake_non_identity_waker_by_ref(data: *const ()) {
            // SAFETY: `wake_by_ref` only borrows the live boxed data pointer.
            let data = unsafe { &*data.cast::<NonIdentityWakerData>() };
            data.0.fetch_add(1, Ordering::Relaxed);
        }

        unsafe fn drop_non_identity_waker(data: *const ()) {
            // SAFETY: `drop` consumes exactly one boxed data pointer.
            drop(unsafe { Box::from_raw(data.cast_mut().cast::<NonIdentityWakerData>()) });
        }

        static NON_IDENTITY_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
            clone_non_identity_waker,
            wake_non_identity_waker,
            wake_non_identity_waker_by_ref,
            drop_non_identity_waker,
        );

        fn non_identity_waker(wake_count: Arc<AtomicUsize>) -> Waker {
            // SAFETY: The vtable maintains one Box owner per RawWaker clone,
            // consumes it from `wake`/`drop`, and only borrows it by reference.
            unsafe { Waker::from_raw(non_identity_raw_waker(wake_count)) }
        }

        struct LockProbe {
            backoff: Weak<AcceptBackoff>,
            lock_was_available: AtomicBool,
        }

        impl LockProbe {
            fn record(&self) {
                let lock_was_available = self
                    .backoff
                    .upgrade()
                    .is_some_and(|backoff| backoff.state.try_lock().is_ok());
                self.lock_was_available
                    .store(lock_was_available, Ordering::Relaxed);
            }
        }

        impl Wake for LockProbe {
            fn wake(self: Arc<Self>) {
                self.record();
            }

            fn wake_by_ref(self: &Arc<Self>) {
                self.record();
            }
        }

        struct LockDropProbe {
            backoff: Weak<AcceptBackoff>,
            lock_was_available: Arc<AtomicBool>,
        }

        impl Wake for LockDropProbe {
            fn wake(self: Arc<Self>) {}
        }

        impl Drop for LockDropProbe {
            fn drop(&mut self) {
                let lock_was_available = self
                    .backoff
                    .upgrade()
                    .is_some_and(|backoff| backoff.state.try_lock().is_ok());
                self.lock_was_available
                    .store(lock_was_available, Ordering::Relaxed);
            }
        }

        fn unique_socket_path() -> (PathBuf, SocketCleanup) {
            let socket_id = NEXT_SOCKET_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "tokio-uds-compat-{}-{socket_id}.sock",
                std::process::id(),
            ));
            match std::fs::remove_file(&path) {
                Ok(()) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => panic!("remove stale Windows AF_UNIX socket file: {error}"),
            }
            let cleanup = SocketCleanup(path.clone());
            (path, cleanup)
        }

        #[test]
        fn accept_returns_non_would_block_errors_without_retrying() {
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);
            let mut injected_error = Some(io::Error::new(
                ErrorKind::ConnectionAborted,
                "injected accept failure",
            ));

            let result = poll_accept_raw::<()>(
                &mut cx,
                || {
                    Err(injected_error
                        .take()
                        .expect("accept should be attempted exactly once"))
                },
                |_| panic!("readiness should not be polled after a non-WouldBlock error"),
            );

            let AcceptPoll::Ready(Err(error)) = result else {
                panic!("non-WouldBlock accept error should be returned immediately");
            };
            assert_eq!(error.kind(), ErrorKind::ConnectionAborted);
            assert_eq!(error.to_string(), "injected accept failure");
        }

        #[test]
        fn accept_rearms_readiness_after_dropped_peer() {
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);
            let mut accept_attempts = 0;
            let mut readiness_polls = 0;

            let result = poll_accept_raw::<()>(
                &mut cx,
                || {
                    accept_attempts += 1;
                    Err(io::Error::from(ErrorKind::WouldBlock))
                },
                |_| {
                    readiness_polls += 1;
                    match readiness_polls {
                        // A peer connected and dropped after the listener registered
                        // interest, leaving a readiness edge but nothing to accept.
                        1 => Poll::Ready(Ok(())),
                        // The listener must register again before parking.
                        2 => Poll::Pending,
                        _ => panic!("readiness should be polled exactly twice"),
                    }
                },
            );

            assert!(matches!(result, AcceptPoll::Pending));
            assert_eq!(accept_attempts, 2);
            assert_eq!(readiness_polls, 2);
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn accept_backs_off_when_readiness_cannot_rearm() {
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);
            let mut accept_attempts = 0;
            let mut readiness_polls = 0;

            let result = poll_accept_raw::<()>(
                &mut cx,
                || {
                    accept_attempts += 1;
                    Err(io::Error::from(ErrorKind::WouldBlock))
                },
                |_| {
                    readiness_polls += 1;
                    Poll::Ready(Ok(()))
                },
            );

            assert!(matches!(result, AcceptPoll::Backoff));
            assert_eq!(accept_attempts, 2);
            assert_eq!(readiness_polls, 2);
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 0);

            let next_poll = poll_accept_raw(
                &mut cx,
                || Ok("accepted after raced readiness"),
                |_| panic!("accept should run before readiness on the next poll"),
            );
            assert!(matches!(
                next_poll,
                AcceptPoll::Ready(Ok("accepted after raced readiness"))
            ));
        }

        #[test]
        fn accept_readiness_rearm_cancels_stale_backoff() {
            let (path, _cleanup) = unique_socket_path();
            let mut listener = UnixListener::bind(&path).expect("bind Windows AF_UNIX listener");
            listener.accept_backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(listener.accept_backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            assert!(matches!(listener.poll_accept(&mut cx), Poll::Pending));
            {
                let state = listener.accept_backoff.lock_state();
                assert!(state.timer.is_unarmed());
                assert_eq!(state.next_delay, Duration::from_secs(5));
            }

            let (readiness_tx, readiness_rx) = mpsc::channel();
            let readiness_waker = Waker::from(Arc::new(WakeSender(readiness_tx)));
            let mut readiness_cx = Context::from_waker(&readiness_waker);
            assert!(matches!(
                listener.poll_accept(&mut readiness_cx),
                Poll::Pending
            ));
            assert!(listener.accept_backoff.lock_state().timer.is_unarmed());
            let _client = async_io::block_on(UnixStream::connect(&path))
                .expect("queue Windows AF_UNIX client after accept backoff");
            readiness_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("readiness should wake a listener re-armed after timer expiry");

            assert!(matches!(
                listener.poll_accept(&mut readiness_cx),
                Poll::Ready(Ok(_))
            ));
        }

        #[test]
        fn consuming_elapsed_timer_wakes_latest_direct_poller() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_millis(100), Duration::from_millis(100));
            let (first_tx, first_rx) = mpsc::channel();
            let first_waker = Waker::from(Arc::new(WakeSender(first_tx)));
            let mut first_cx = Context::from_waker(&first_waker);
            let timer = expect_armed_timer(backoff.arm_and_poll(&mut first_cx));
            assert_eq!(timer.initial_poll, AcceptTimerPoll::Pending);
            first_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("timer should wake its initial poller");

            let latest_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let latest_waker = Waker::from(latest_wake_count.clone());
            backoff.register(&latest_waker);

            assert_eq!(
                backoff.poll_timer(&mut first_cx, timer.timer.clone()),
                AcceptTimerPoll::Elapsed
            );
            assert_eq!(latest_wake_count.0.load(Ordering::Relaxed), 1);
            assert!(backoff.lock_state().timer.is_unarmed());
            assert_eq!(
                backoff.poll_timer(&mut first_cx, timer.timer),
                AcceptTimerPoll::Invalidated,
                "a detached completed one-shot timer must not be polled again"
            );
        }

        #[test]
        fn contended_pending_timer_poll_wakes_latest_waiter() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(5));
            let first_waker = noop_waker();
            let mut first_cx = Context::from_waker(&first_waker);
            let timer = expect_armed_timer(backoff.arm_and_poll(&mut first_cx)).timer;
            {
                let mut state = backoff.lock_state();
                state
                    .timer
                    .armed_mut()
                    .expect("timer should remain armed")
                    .poll_state = AcceptTimerPollState::Polling;
            }

            let latest_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let latest_waker = Waker::from(latest_wake_count.clone());
            let mut latest_cx = Context::from_waker(&latest_waker);
            backoff.register(&latest_waker);

            assert_eq!(
                backoff.poll_timer(&mut latest_cx, timer.clone()),
                AcceptTimerPoll::Pending
            );
            assert!(matches!(
                backoff
                    .lock_state()
                    .timer
                    .armed()
                    .expect("timer should remain armed")
                    .poll_state,
                AcceptTimerPollState::RepollRequested
            ));

            assert_eq!(
                backoff.finish_timer_poll(&mut first_cx, &timer, Poll::Pending),
                AcceptTimerPoll::Pending
            );
            assert_eq!(latest_wake_count.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                backoff
                    .lock_state()
                    .timer
                    .armed()
                    .expect("timer should remain armed")
                    .poll_state,
                AcceptTimerPollState::Idle
            ));
        }

        #[test]
        fn accept_attempts_ready_peer_and_resets_shared_backoff() {
            let (path, _cleanup) = unique_socket_path();
            let mut listener = UnixListener::bind(&path).expect("bind Windows AF_UNIX listener");
            listener.accept_backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(listener.accept_backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            {
                let backoff = listener.accept_backoff.lock_state();
                assert!(backoff.timer.is_armed());
                assert_eq!(backoff.next_delay, Duration::from_secs(10));
            }
            assert_eq!(
                expect_armed_timer(listener.accept_backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            assert_eq!(
                listener.accept_backoff.lock_state().next_delay,
                Duration::from_secs(10),
                "polling an armed timer must preserve its escalated delay"
            );

            let _client = async_io::block_on(UnixStream::connect(&path))
                .expect("queue Windows AF_UNIX client during accept backoff");

            assert!(matches!(listener.poll_accept(&mut cx), Poll::Ready(Ok(_))));
            let backoff = listener.accept_backoff.lock_state();
            assert!(backoff.timer.is_unarmed());
            assert_eq!(
                backoff.next_delay,
                Duration::from_secs(5),
                "accept progress must reset the next backoff to its initial delay"
            );
        }

        #[test]
        fn accept_backoff_grows_exponentially_to_a_cap() {
            assert_eq!(
                next_accept_backoff(Duration::from_millis(1), Duration::from_millis(100)),
                Duration::from_millis(2)
            );
            assert_eq!(
                next_accept_backoff(Duration::from_millis(60), Duration::from_millis(100)),
                Duration::from_millis(100)
            );
            assert_eq!(
                next_accept_backoff(Duration::from_millis(100), Duration::from_millis(100)),
                Duration::from_millis(100)
            );
        }

        #[test]
        fn stale_timer_candidate_cannot_cross_reset_aba() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let (generation, delay) = {
                let state = backoff.lock_state();
                (state.generation, state.next_delay)
            };
            let stale_candidate = Arc::new(Mutex::new(Timer::after(delay)));
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);

            let _current = expect_armed_timer(backoff.arm_and_poll(&mut cx));
            backoff.reset(None);
            assert_eq!(backoff.lock_state().next_delay, delay);

            assert!(
                backoff
                    .install_timer_candidate(generation, delay, stale_candidate)
                    .is_none(),
                "a generation change must reject a candidate even when the delay cycles back"
            );
            assert!(backoff.lock_state().timer.is_unarmed());
        }

        #[test]
        fn timer_creation_is_reserved_before_reactor_registration() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));

            let timer = backoff.get_or_create_timer_with(|delay| {
                assert!(
                    backoff
                        .get_or_create_timer_with(|_| {
                            panic!("a contender must not construct another timer")
                        })
                        .is_none(),
                    "a caller observing an in-flight creation relies on its wake"
                );
                Arc::new(Mutex::new(Timer::after(delay)))
            });

            assert!(timer.is_some());
            assert!(backoff.lock_state().timer.is_armed());
        }

        #[test]
        fn timer_creation_panic_clears_reservation_and_wakes_contender() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(5));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());

            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                backoff.get_or_create_timer_with(|_| {
                    backoff.register(&waker);
                    assert!(
                        backoff
                            .get_or_create_timer_with(|_| {
                                panic!("a contender must not construct another timer")
                            })
                            .is_none()
                    );
                    panic!("injected timer creation panic")
                })
            }));

            assert!(result.is_err());
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);
            assert!(backoff.lock_state().timer.is_unarmed());
            assert!(
                backoff.get_or_create_timer().is_some(),
                "a timer must remain creatable after reservation unwind"
            );
        }

        #[test]
        fn timer_poll_panic_releases_ownership_and_wakes_creation_contender() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(5));
            let first_waker = noop_waker();
            let mut first_cx = Context::from_waker(&first_waker);
            let latest_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let latest_waker = Waker::from(latest_wake_count.clone());
            let mut latest_cx = Context::from_waker(&latest_waker);
            let timer = backoff
                .get_or_create_timer_with(|delay| {
                    backoff.register(&latest_waker);
                    assert!(
                        backoff
                            .get_or_create_timer_with(|_| {
                                panic!("a contender must not construct another timer")
                            })
                            .is_none()
                    );
                    Arc::new(Mutex::new(Timer::after(delay)))
                })
                .expect("creator should install its reserved timer");

            let result = panic::catch_unwind(AssertUnwindSafe(|| {
                backoff.poll_timer_with(&mut first_cx, timer.clone(), |_, _| {
                    panic!("injected timer poll panic")
                })
            }));

            assert!(result.is_err());
            assert_eq!(latest_wake_count.0.load(Ordering::Relaxed), 1);
            assert!(matches!(
                backoff
                    .lock_state()
                    .timer
                    .armed()
                    .expect("timer should remain armed")
                    .poll_state,
                AcceptTimerPollState::Idle
            ));
            assert_eq!(
                backoff.poll_timer(&mut latest_cx, timer),
                AcceptTimerPoll::Pending,
                "a recovered poisoned timer must remain pollable"
            );
        }

        #[test]
        fn elapsed_backoff_with_immediate_readiness_schedules_one_repoll() {
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);

            assert!(matches!(
                finish_accept_backoff::<()>(
                    &mut cx,
                    || Err(io::Error::from(ErrorKind::WouldBlock)),
                    |_| Poll::Ready(Ok(())),
                    |timer_cx| timer_cx.waker().wake_by_ref(),
                ),
                AcceptBackoffPoll::WakePending
            ));
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);

            let mut live_timer_checks = 0;
            assert!(matches!(
                finish_accept_backoff::<()>(
                    &mut cx,
                    || Err(io::Error::from(ErrorKind::WouldBlock)),
                    |_| Poll::Ready(Ok(())),
                    |_| live_timer_checks += 1,
                ),
                AcceptBackoffPoll::WakePending
            ));
            assert_eq!(live_timer_checks, 1);
            assert_eq!(
                wake_count.0.load(Ordering::Relaxed),
                1,
                "a live timer already owns the next wake"
            );
        }

        #[test]
        fn backoff_accepts_before_final_readiness_poll() {
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);

            let result = finish_accept_backoff(
                &mut cx,
                || Ok("accepted before backoff"),
                |_| panic!("readiness should not be polled after accept succeeds"),
                |_| panic!("timer should not be polled after accept succeeds"),
            );

            assert!(matches!(
                result,
                AcceptBackoffPoll::Ready(Ok("accepted before backoff"))
            ));
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn elapsed_backoff_parks_after_final_readiness_registration() {
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);
            let mut accept_attempts = 0;
            let mut readiness_polls = 0;

            let result = finish_accept_backoff::<()>(
                &mut cx,
                || {
                    accept_attempts += 1;
                    Err(io::Error::from(ErrorKind::WouldBlock))
                },
                |_| {
                    readiness_polls += 1;
                    Poll::Pending
                },
                |_| panic!("Pending readiness should own the next wake"),
            );

            assert!(matches!(result, AcceptBackoffPoll::ReadinessPending));
            assert_eq!(accept_attempts, 1);
            assert_eq!(readiness_polls, 1);
            assert_eq!(
                wake_count.0.load(Ordering::Relaxed),
                0,
                "Pending readiness owns the next wake after the final accept"
            );
        }

        #[test]
        fn invalidated_timer_rearms_delay_without_immediate_wake() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let first_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let first_waker = Waker::from(first_wake_count.clone());
            let mut first_cx = Context::from_waker(&first_waker);
            let second_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let second_waker = Waker::from(second_wake_count.clone());
            let armed_timer = expect_armed_timer(backoff.arm_and_poll(&mut first_cx));
            assert_eq!(armed_timer.initial_poll, AcceptTimerPoll::Pending);

            backoff.reset(None);
            backoff.register(&second_waker);
            assert_eq!(
                backoff.poll_timer(&mut first_cx, armed_timer.timer),
                AcceptTimerPoll::Invalidated
            );
            backoff.ensure_backoff_wake(&mut first_cx);

            assert_eq!(first_wake_count.0.load(Ordering::Relaxed), 1);
            assert_eq!(
                second_wake_count.0.load(Ordering::Relaxed),
                0,
                "timer invalidation must not turn the delay into an immediate wake"
            );
            assert!(backoff.lock_state().timer.is_armed());
        }

        #[test]
        fn direct_completion_clears_and_resets_backoff() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            backoff.reset(Some(&waker));

            let state = backoff.lock_state();
            assert!(state.timer.is_unarmed());
            assert!(state.waiter.is_none());
            assert_eq!(state.next_delay, Duration::from_secs(5));
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn direct_completion_after_wake_resets_backoff() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            backoff.wake_latest();
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);

            backoff.reset(Some(&waker));

            let state = backoff.lock_state();
            assert!(state.timer.is_unarmed());
            assert!(state.waiter.is_none());
            assert_eq!(state.next_delay, Duration::from_secs(5));
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);
        }

        #[test]
        fn direct_completion_tolerates_non_identity_preserving_waker_clone() {
            let backoff = AcceptBackoff::new();
            let wake_count = Arc::new(AtomicUsize::new(0));
            let waker = non_identity_waker(wake_count.clone());
            let cloned_waker = waker.clone();
            assert!(!waker.will_wake(&cloned_waker));
            drop(cloned_waker);
            backoff.register(&waker);

            backoff.reset(Some(&waker));

            assert_eq!(wake_count.load(Ordering::Relaxed), 1);
            assert!(backoff.lock_state().waiter.is_none());
        }

        #[test]
        fn async_accept_progress_wakes_displaced_direct_poller() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            backoff.reset(None);

            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);
            let state = backoff.lock_state();
            assert!(state.timer.is_unarmed());
            assert!(state.waiter.is_none());
            assert_eq!(state.next_delay, Duration::from_secs(5));
        }

        #[test]
        fn poll_accept_readiness_uses_one_stable_bridge_waker() {
            let backoff = AcceptBackoff::new();
            let first_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let first_waker = Waker::from(first_wake_count.clone());
            let second_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let second_waker = Waker::from(second_wake_count.clone());
            backoff.register(&first_waker);
            backoff.register(&second_waker);

            let first_broadcast = backoff.with_broadcast_waker(|cx| cx.waker().clone());
            let second_broadcast = backoff.with_broadcast_waker(|cx| cx.waker().clone());
            assert!(
                first_broadcast.will_wake(&second_broadcast),
                "concurrent readiness polls must not replace async-io's waker"
            );

            first_broadcast.wake();
            assert_eq!(first_wake_count.0.load(Ordering::Relaxed), 0);
            assert_eq!(second_wake_count.0.load(Ordering::Relaxed), 1);
        }

        #[test]
        fn readiness_rearm_without_timer_preserves_source_waiter() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            backoff.register(&waker);

            backoff.readiness_rearmed();
            {
                let state = backoff.lock_state();
                assert!(state.timer.is_unarmed());
                assert!(state.waiter.is_some());
                assert_eq!(state.next_delay, Duration::from_secs(5));
            }

            backoff.with_broadcast_waker(|broadcast_cx| broadcast_cx.waker().wake_by_ref());
            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);
        }

        #[test]
        fn readiness_rearm_wakes_newer_timer_waiter() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let first_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let first_waker = Waker::from(first_wake_count.clone());
            let mut first_cx = Context::from_waker(&first_waker);
            let _timer = expect_armed_timer(backoff.arm_and_poll(&mut first_cx));
            let latest_wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let latest_waker = Waker::from(latest_wake_count.clone());
            backoff.register(&latest_waker);

            backoff.readiness_rearmed();

            assert_eq!(first_wake_count.0.load(Ordering::Relaxed), 0);
            assert_eq!(latest_wake_count.0.load(Ordering::Relaxed), 1);
            let state = backoff.lock_state();
            assert!(state.timer.is_unarmed());
            assert!(state.waiter.is_none());
            assert_eq!(state.next_delay, Duration::from_secs(5));
        }

        #[test]
        fn readiness_rearm_wakes_timer_waiter_with_same_waker() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(10));
            let wake_count = Arc::new(WakeCounter(AtomicUsize::new(0)));
            let waker = Waker::from(wake_count.clone());
            let mut cx = Context::from_waker(&waker);
            let _timer = expect_armed_timer(backoff.arm_and_poll(&mut cx));

            backoff.readiness_rearmed();

            assert_eq!(wake_count.0.load(Ordering::Relaxed), 1);
            let state = backoff.lock_state();
            assert!(state.timer.is_unarmed());
            assert!(state.waiter.is_none());
            assert_eq!(state.next_delay, Duration::from_secs(5));
        }

        #[test]
        fn accept_registers_concurrent_futures_independently() {
            let (path, _cleanup) = unique_socket_path();
            let listener = UnixListener::bind(&path).expect("bind Windows AF_UNIX listener");
            let (first_tx, first_rx) = mpsc::channel();
            let first_waker = Waker::from(Arc::new(WakeSender(first_tx)));
            let mut first_cx = Context::from_waker(&first_waker);
            let (second_tx, second_rx) = mpsc::channel();
            let second_waker = Waker::from(Arc::new(WakeSender(second_tx)));
            let mut second_cx = Context::from_waker(&second_waker);
            let mut first_accept = Box::pin(listener.accept());
            let mut second_accept = Box::pin(listener.accept());

            assert!(first_accept.as_mut().poll(&mut first_cx).is_pending());
            assert!(second_accept.as_mut().poll(&mut second_cx).is_pending());
            let _first_client = async_io::block_on(UnixStream::connect(&path))
                .expect("queue first concurrent Windows AF_UNIX client");
            let _second_client = async_io::block_on(UnixStream::connect(&path))
                .expect("queue second concurrent Windows AF_UNIX client");

            first_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("first concurrent accept future should receive a readiness wake");
            second_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("second concurrent accept future should receive a readiness wake");

            assert!(matches!(
                first_accept.as_mut().poll(&mut first_cx),
                Poll::Ready(Ok(_))
            ));
            assert!(matches!(
                second_accept.as_mut().poll(&mut second_cx),
                Poll::Ready(Ok(_))
            ));
        }

        #[test]
        fn accept_backoff_timer_does_not_retain_listener_state() {
            let backoff =
                AcceptBackoff::with_delays(Duration::from_secs(5), Duration::from_secs(5));
            let weak_backoff = Arc::downgrade(&backoff);
            let waker = noop_waker();
            let mut cx = Context::from_waker(&waker);

            assert_eq!(
                expect_armed_timer(backoff.arm_and_poll(&mut cx)).initial_poll,
                AcceptTimerPoll::Pending
            );
            assert!(backoff.lock_state().timer.is_armed());
            drop(backoff);

            assert!(
                weak_backoff.upgrade().is_none(),
                "the timer's broadcast waker must not form an Arc cycle"
            );
        }

        #[test]
        fn accept_bridge_wakes_after_releasing_state_lock() {
            let backoff = AcceptBackoff::new();
            let lock_probe = Arc::new(LockProbe {
                backoff: Arc::downgrade(&backoff),
                lock_was_available: AtomicBool::new(false),
            });
            let waker = Waker::from(lock_probe.clone());
            backoff.register(&waker);

            backoff.wake_latest();

            assert!(
                lock_probe.lock_was_available.load(Ordering::Relaxed),
                "the bridge must release state before invoking a caller waker"
            );
        }

        #[test]
        fn replacing_waiter_drops_displaced_waker_after_state_unlock() {
            let backoff = AcceptBackoff::new();
            let lock_was_available = Arc::new(AtomicBool::new(false));
            let probe_waker = Waker::from(Arc::new(LockDropProbe {
                backoff: Arc::downgrade(&backoff),
                lock_was_available: lock_was_available.clone(),
            }));
            backoff.register(&probe_waker);
            drop(probe_waker);

            backoff.register(&noop_waker());

            assert!(
                lock_was_available.load(Ordering::Relaxed),
                "a displaced caller waker must be dropped after releasing shared state"
            );
        }

        #[test]
        fn accept_drains_connections_already_queued_in_the_backlog() {
            let (path, _cleanup) = unique_socket_path();

            async_io::block_on(async {
                let listener = UnixListener::bind(&path).expect("bind Windows AF_UNIX listener");

                let mut queued_clients = Vec::with_capacity(QUEUED_CLIENTS);
                for _ in 0..QUEUED_CLIENTS {
                    let stream = UnixStream::connect(&path)
                        .await
                        .expect("queue Windows AF_UNIX client");
                    queued_clients.push(stream);
                }

                let drain_backlog = async {
                    for _ in 0..QUEUED_CLIENTS {
                        listener.accept().await.expect("accept queued client");
                    }
                };
                let deadline = async_io::Timer::after(BACKLOG_DRAIN_DEADLINE);
                futures::pin_mut!(drain_backlog, deadline);

                match futures::future::select(drain_backlog, deadline).await {
                    Either::Left(((), _deadline)) => {}
                    Either::Right((_elapsed, _drain_backlog)) => {
                        panic!(
                            "timed out after {BACKLOG_DRAIN_DEADLINE:?} while draining queued clients"
                        )
                    }
                }
                drop(queued_clients);
            });
        }
    }
}

#[cfg(windows)]
pub use self::windows::OwnedReadHalf;
#[cfg(windows)]
pub use self::windows::OwnedWriteHalf;
#[cfg(windows)]
pub use self::windows::UnixListener;
#[cfg(windows)]
pub use self::windows::UnixStream;
