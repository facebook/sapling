/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::*;
use futures::executor::{spawn, Notify, NotifyHandle, Spawn};
use futures_ext::FutureExt;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use std::usize;

// A simple operation, also keep track of the number of times invoked
struct Upperer<'a>(&'a AtomicUsize);

impl<'a> Filler for Upperer<'a> {
    type Key = String;
    type Value = Result<String, ()>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(key.to_uppercase())
    }
}

#[test]
fn simple() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Upperer(&count), 1);

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    assert_eq!(count.load(Ordering::Relaxed), 0);

    let v = c.get("foo").wait().unwrap();
    assert_eq!(v, "FOO");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert!(!c.is_empty());
    assert_eq!(c.len(), 1);

    let v = c.get("foo").wait().unwrap();
    assert_eq!(v, "FOO");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert_eq!(c.len(), 1);

    let v = c.get("bar").wait().unwrap();
    assert_eq!(v, "BAR");
    assert_eq!(count.load(Ordering::Relaxed), 2);
    assert_eq!(c.len(), 2);
}

#[test]
fn clear() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Upperer(&count), 1);

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    assert_eq!(count.load(Ordering::Relaxed), 0);

    let v = c.get("foo").wait().unwrap();
    assert_eq!(v, "FOO");
    assert_eq!(count.load(Ordering::Relaxed), 1);
    assert!(!c.is_empty());
    assert_eq!(c.len(), 1);

    c.clear();

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    let v = c.get("foo").wait().unwrap();
    assert_eq!(v, "FOO");
    assert_eq!(count.load(Ordering::Relaxed), 2);
    assert!(!c.is_empty());
    assert_eq!(c.len(), 1);
}

#[test]
fn size_limit() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::with_limits_and_shards("test", Upperer(&count), 3, usize::MAX, 1);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);

    let v2 = c.get("goodbye").wait().unwrap();
    assert_eq!(v2, "GOODBYE", "c={:#?}", c);
    assert_eq!(c.len(), 2, "c={:#?}", c);

    let v3 = c.get("world").wait().unwrap();
    assert_eq!(v3, "WORLD", "c={:#?}", c);
    assert_eq!(c.len(), 3, "c={:#?}", c);

    let v4 = c.get("ungulate").wait().unwrap();
    assert_eq!(v4, "UNGULATE", "c={:#?}", c);
    assert_eq!(c.len(), 3, "c={:#?}", c);
}

#[test]
fn weight_limit_simple() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::with_limits_and_shards("test", Upperer(&count), 3, 100, 1);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    // Note - this test can fail if "HELLO" was allocated differently
    // inside asyncmemo or in the test. If that the case, then fix the test or disable it.
    let expected_weight = String::from("HELLO").get_weight();
    assert_eq!(c.total_weight(), expected_weight, "c={:#?}", c);
}

#[test]
fn weight_limit_eviction() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::with_limits_and_shards("test", Upperer(&count), 1, usize::MAX, 1);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    let expected_weight = String::from("HELLO").get_weight();
    assert_eq!(c.total_weight(), expected_weight, "c={:#?}", c);

    let v1 = c.get("hell").wait().unwrap();
    assert_eq!(v1, "HELL", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    let expected_weight = String::from("HELL").get_weight();
    assert_eq!(c.total_weight(), expected_weight, "c={:#?}", c);
}

#[derive(Debug)]
struct Delay<V> {
    remains: usize,
    v: Option<Result<V, ()>>,
}

impl<V> Future for Delay<V> {
    type Item = V;
    type Error = ();

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.remains == 0 {
            match self.v.take() {
                None => Err(()),
                Some(Ok(v)) => Ok(Async::Ready(v)),
                Some(Err(e)) => Err(e),
            }
        } else {
            self.remains -= 1;
            Ok(Async::NotReady)
        }
    }
}

#[derive(Debug)]
struct Delayed<'a>(&'a AtomicUsize, usize);

impl<'a> Filler for Delayed<'a> {
    type Key = String;
    type Value = Delay<String>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        self.0.fetch_add(1, Ordering::Relaxed);
        Delay {
            remains: self.1,
            v: Some(Ok(key.to_uppercase())),
        }
    }
}

struct DummyNotify {}

impl Notify for DummyNotify {
    fn notify(&self, _id: usize) {}
}

#[test]
fn delayed() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Delayed(&count, 5), 1);

    let notify_handle = NotifyHandle::from(Arc::new(DummyNotify {}));
    let dummy_id = 0;

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    assert_eq!(count.load(Ordering::Relaxed), 0);

    let mut v = spawn(c.get("foo"));

    assert_eq!(count.load(Ordering::Relaxed), 0);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::Ready("FOO".into())),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::Ready("FOO".into())),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

struct Fib(Arc<AtomicUsize>);

impl Filler for Fib {
    type Key = u32;
    type Value = BoxFuture<u32, ()>;

    fn fill(&self, cache: &Asyncmemo<Self>, key: &u32) -> Self::Value {
        self.0.fetch_add(1, Ordering::Relaxed);

        let key = *key;

        if key == 1 {
            let f = Delay::<u32> {
                remains: 1,
                v: Some(Ok(1)),
            };
            Box::new(f) as BoxFuture<u32, ()>
        } else {
            let f = cache.get(key - 1).and_then(move |f| Delay {
                remains: 1,
                v: Some(Ok(key + f)),
            });
            Box::new(f) as BoxFuture<u32, ()>
        }
    }
}

#[test]
fn fibonacci() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = Asyncmemo::new_unbounded("test", Fib(count.clone()), 1);

    let notify_handle = NotifyHandle::from(Arc::new(DummyNotify {}));
    let dummy_id = 0;
    {
        let mut fib = spawn(c.get(1u32));

        assert_eq!(
            fib.poll_future_notify(&notify_handle, dummy_id),
            Ok(Async::NotReady)
        );
        assert_eq!(count.load(Ordering::Relaxed), 1);

        assert_eq!(
            fib.poll_future_notify(&notify_handle, dummy_id),
            Ok(Async::Ready(1))
        );
        assert_eq!(count.load(Ordering::Relaxed), 1);

        assert_eq!(
            fib.poll_future_notify(&notify_handle, dummy_id),
            Ok(Async::Ready(1))
        );
        assert_eq!(count.load(Ordering::Relaxed), 1);

        println!(
            "1: fib.poll()={:?}",
            fib.poll_future_notify(&notify_handle, dummy_id)
        );
    }

    {
        let mut fib = spawn(c.get(1u32));

        assert_eq!(
            fib.poll_future_notify(&notify_handle, dummy_id),
            Ok(Async::Ready(1))
        );
        assert_eq!(count.load(Ordering::Relaxed), 1);

        println!(
            "1: fib.poll()={:?}",
            fib.poll_future_notify(&notify_handle, dummy_id)
        );
    }

    {
        let mut fib = spawn(c.get(2u32));

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("2: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::NotReady));
        assert_eq!(count.load(Ordering::Relaxed), 2);

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("2: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::Ready(3)));
        assert_eq!(count.load(Ordering::Relaxed), 2);

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("2: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::Ready(3)));
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    {
        println!("future 4");
        let mut fib = spawn(c.get(4u32));

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("4: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::NotReady));
        assert_eq!(count.load(Ordering::Relaxed), 4);

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("4: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::Ready(10)));
        assert_eq!(count.load(Ordering::Relaxed), 4);

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("4: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::Ready(10)));
        assert_eq!(count.load(Ordering::Relaxed), 4);
    }
}

#[derive(Debug)]
struct Fails<'a>(&'a AtomicUsize);

impl<'a> Filler for Fails<'a> {
    type Key = String;
    type Value = Delay<String>;

    fn fill(&self, _cache: &Asyncmemo<Self>, _: &Self::Key) -> Self::Value {
        self.0.fetch_add(1, Ordering::Relaxed);
        Delay {
            remains: 3,
            v: Some(Err(())),
        }
    }
}

#[test]
fn failing() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Fails(&count), 1);

    let notify_handle = NotifyHandle::from(Arc::new(DummyNotify {}));
    let dummy_id = 0;

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    assert_eq!(count.load(Ordering::Relaxed), 0);

    let mut v = spawn(c.get("foo"));
    assert_eq!(count.load(Ordering::Relaxed), 0);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Err(()),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    // retry
    assert_eq!(
        v.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v
    );
    assert_eq!(count.load(Ordering::Relaxed), 2);
}

#[test]
fn multiwait() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Delayed(&count, 5), 1);

    let notify_handle = NotifyHandle::from(Arc::new(DummyNotify {}));
    let dummy_id = 0;

    assert!(c.is_empty());
    assert_eq!(c.len(), 0);

    let mut v1 = spawn(c.get("foo"));
    assert_eq!(count.load(Ordering::Relaxed), 0);
    let mut v2 = spawn(c.get("foo"));
    assert_eq!(count.load(Ordering::Relaxed), 0);

    // polling on either future advances the state machine until its complete

    assert_eq!(
        v1.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v1
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v2.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v2
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v1.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v1
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v2.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v2
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v1.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::NotReady),
        "v={:#?}",
        v1
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v2.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::Ready("FOO".into())),
        "v={:#?}",
        v2
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);

    assert_eq!(
        v1.poll_future_notify(&notify_handle, dummy_id),
        Ok(Async::Ready("FOO".into())),
        "v={:#?}",
        v1
    );
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

struct SimpleNotify {
    pub was_notified: Mutex<bool>,
}

impl SimpleNotify {
    fn new() -> Self {
        SimpleNotify {
            was_notified: Mutex::new(false),
        }
    }
}

impl Notify for SimpleNotify {
    fn notify(&self, _id: usize) {
        *self.was_notified.lock().unwrap() = true;
    }
}

struct SpawnedFutureAndNotify<T> {
    spawned: Spawn<T>,
    simple_notify: Arc<SimpleNotify>,
    notify_handle: NotifyHandle,
}

impl<T> SpawnedFutureAndNotify<T>
where
    T: Future,
{
    fn new(fut: T) -> Self {
        let simple_notify = Arc::new(SimpleNotify::new());
        SpawnedFutureAndNotify {
            spawned: spawn(fut),
            simple_notify: simple_notify.clone(),
            notify_handle: NotifyHandle::from(simple_notify),
        }
    }

    fn poll(&mut self) -> Poll<<T as Future>::Item, <T as Future>::Error> {
        self.spawned.poll_future_notify(&self.notify_handle, 0)
    }

    fn was_notified(&self) -> bool {
        *self.simple_notify.was_notified.lock().unwrap()
    }
}

#[test]
fn timer_multiwait() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded("test", Delayed(&count, 2), 1);

    let mut v1 = SpawnedFutureAndNotify::new(c.get("foo"));
    let mut v2 = SpawnedFutureAndNotify::new(c.get("foo"));
    assert_eq!(v1.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::Ready(String::from("FOO"))));
    assert!(v1.was_notified());
}

struct Timered {
    res: Vec<Result<String, ()>>,
    cur_result: RefCell<usize>,
    remains: usize,
}

impl Timered {
    fn new(res: Vec<Result<String, ()>>, remains: usize) -> Self {
        Timered {
            res,
            cur_result: RefCell::new(0),
            remains,
        }
    }
}

impl Filler for Timered {
    type Key = String;
    type Value = Delay<String>;

    fn fill(&self, _cache: &Asyncmemo<Self>, _key: &Self::Key) -> Self::Value {
        let index = *self.cur_result.borrow();
        let res = self.res.get(index).unwrap().clone();
        *self.cur_result.borrow_mut() = index + 1;
        Delay {
            remains: self.remains,
            v: Some(res),
        }
    }
}

#[test]
fn test_timer_future() {
    let c = Asyncmemo::new_unbounded("test", Timered::new(vec![Ok("RES".into())], 2), 1);
    let mut v1 = SpawnedFutureAndNotify::new(c.get("res"));
    let mut v2 = SpawnedFutureAndNotify::new(c.get("res"));
    assert_eq!(v1.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::NotReady));
    assert_eq!(v1.poll(), Ok(Async::Ready(String::from("RES"))));
    assert_eq!(v2.poll(), Ok(Async::Ready(String::from("RES"))));
    assert!(v1.was_notified());
    assert!(v2.was_notified());
}

#[test]
fn test_timer_future_many_futures() {
    let c = Asyncmemo::new_unbounded("test", Timered::new(vec![Ok("RES".into())], 9), 1);
    let mut futs: Vec<_> = (1..10)
        .map(|_| {
            let mut f = SpawnedFutureAndNotify::new(c.get("res"));
            assert_eq!(f.poll(), Ok(Async::NotReady));
            f
        })
        .collect();
    assert_eq!(futs[0].poll(), Ok(Async::Ready(String::from("RES"))));
    for f in futs {
        assert!(f.was_notified());
    }
}

#[test]
fn test_drop() {
    {
        let c = Asyncmemo::new_unbounded("test", Timered::new(vec![Ok("RES".into())], 2), 2);
        let mut v1 = SpawnedFutureAndNotify::new(c.get("res"));
        let mut v2 = SpawnedFutureAndNotify::new(c.get("res"));
        assert_eq!(v1.poll(), Ok(Async::NotReady));
        assert_eq!(v2.poll(), Ok(Async::NotReady));
        std::mem::drop(v1);
        assert_eq!(v2.poll(), Ok(Async::Ready(String::from("RES"))));
        assert!(v2.was_notified());
    }

    {
        // Vice-versa: drop the future that was polled second, check that first was notified
        let c = Asyncmemo::new_unbounded("test", Timered::new(vec![Ok("RES".into())], 2), 1);
        let mut v1 = SpawnedFutureAndNotify::new(c.get("res"));
        let mut v2 = SpawnedFutureAndNotify::new(c.get("res"));
        assert_eq!(v1.poll(), Ok(Async::NotReady));
        assert_eq!(v2.poll(), Ok(Async::NotReady));
        std::mem::drop(v2);
        assert_eq!(v1.poll(), Ok(Async::Ready(String::from("RES"))));
        assert!(v1.was_notified());
    }
}

#[test]
fn test_poll_after_sporadic_failure() {
    let c = Asyncmemo::new_unbounded("test", Timered::new(vec![Err(()), Ok("RES".into())], 2), 1);
    let mut v1 = SpawnedFutureAndNotify::new(c.get("res"));
    let mut v2 = SpawnedFutureAndNotify::new(c.get("res"));
    assert_eq!(v1.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::NotReady));
    // Sporadic error has failed, polling second future should succeed
    assert_eq!(v1.poll(), Err(()));
    assert!(v1.was_notified());
    assert!(v2.was_notified());
    assert_eq!(v2.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::NotReady));
    assert_eq!(v2.poll(), Ok(Async::Ready("RES".into())));
}

struct SlowPollUpperrer {
    res: Result<String, String>,
}

impl SlowPollUpperrer {
    fn new(res: Result<String, String>) -> Self {
        SlowPollUpperrer { res }
    }
}

impl Filler for SlowPollUpperrer {
    type Key = String;
    type Value = futures_ext::BoxFuture<String, String>;

    fn fill(&self, _cache: &Asyncmemo<Self>, _key: &Self::Key) -> Self::Value {
        thread::sleep(Duration::from_millis(100));
        return self.res.clone().into_future().boxify();
    }
}

#[test]
fn slow_poll_success() {
    // Two futures poll at roughly the same time.
    // Poll is slow, so one future does the poll, another goes to the Polling state.
    // Make sure that second future is woken up after the first one succeed
    let c = Asyncmemo::new_unbounded("test", SlowPollUpperrer::new(Ok("RES".into())), 1);

    let t1 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    let t2 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
}

#[test]
fn slow_poll_err() {
    // Two futures poll at roughly the same time.
    // Poll is slow, so one future does the poll, another goes to the Polling state.
    // Make sure that second future is woken up after the first one errored
    let c = Asyncmemo::new_unbounded("test", SlowPollUpperrer::new(Err("RES".into())), 1);
    fn assert_send<T: Send>(_t: &T) {}
    assert_send(&c);

    let t1 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_err());
        }
    });

    let t2 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_err());
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();
}

struct SignalBasedUpperrer {
    res: Result<String, String>,
    signal: Arc<AtomicBool>,
}

impl SignalBasedUpperrer {
    fn new(res: Result<String, String>, signal: Arc<AtomicBool>) -> Self {
        SignalBasedUpperrer { res, signal }
    }
}

impl Filler for SignalBasedUpperrer {
    type Key = String;
    type Value = futures_ext::BoxFuture<String, String>;

    fn fill(&self, _cache: &Asyncmemo<Self>, key: &Self::Key) -> Self::Value {
        if !key.starts_with("skipwaiting") {
            loop {
                if self.signal.load(Ordering::SeqCst) {
                    break;
                }
            }
        }
        return self.res.clone().into_future().boxify();
    }
}

#[test]
fn slow_poll_invalidate() {
    let signal = Arc::new(AtomicBool::new(false));
    let c = Asyncmemo::new_unbounded(
        "test",
        SignalBasedUpperrer::new(Ok("RES".into()), signal.clone()),
        1,
    );

    let t1 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    let t2 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    // Make sure one future is in Polling, another future Polls.
    thread::sleep(Duration::from_millis(50));
    c.invalidate("res");
    assert_eq!(c.len(), 0);
    // Allow futures to proceed
    signal.store(true, Ordering::SeqCst);

    t1.join().unwrap();
    t2.join().unwrap();
}

#[test]
fn slow_clear_invalidate() {
    let signal = Arc::new(AtomicBool::new(false));
    let c = Asyncmemo::new_unbounded(
        "test",
        SignalBasedUpperrer::new(Ok("RES".into()), signal.clone()),
        1,
    );

    let t1 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    let t2 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    // Make sure one future is in Polling, another future Polls.
    thread::sleep(Duration::from_millis(50));
    c.clear();
    assert_eq!(c.len(), 0);
    // Allow futures to proceed
    signal.store(true, Ordering::SeqCst);

    t1.join().unwrap();
    t2.join().unwrap();
}

#[test]
fn polling_hash_trimming() {
    // Spawn two futures for the same key - one will be in Polling state.
    // Spawn one more future that skips waiting on the signal and evicts the previous futures
    // from the cache. Make sure nothing is deadlocked.
    let signal = Arc::new(AtomicBool::new(false));

    let longskipkey = String::from("skipwaiting");

    let c = Asyncmemo::with_limits_and_shards(
        "test",
        SignalBasedUpperrer::new(Ok("RES".into()), signal.clone()),
        1,
        // Make space exactly for one future and it's result
        longskipkey.get_weight() + String::from("RES").get_weight(),
        1,
    );

    let t1 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    let t2 = thread::spawn({
        let c = c.clone();
        move || {
            let fut = c.get("res");
            assert!(spawn(fut).wait_future().is_ok());
        }
    });

    // Make sure one future is in Polling state, another future polls.
    thread::sleep(Duration::from_millis(50));
    // Evict "res" future
    let fut = c.get(longskipkey);
    assert!(spawn(fut).wait_future().is_ok());
    assert_eq!(c.len(), 1);
    // Allow futures to proceed
    signal.store(true, Ordering::SeqCst);

    t1.join().unwrap();
    t2.join().unwrap();
}
