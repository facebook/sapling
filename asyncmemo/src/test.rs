// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::*;
use futures::executor::{spawn, Notify, NotifyHandle};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
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
    let c = Asyncmemo::new_unbounded(Upperer(&count));

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
    let c = Asyncmemo::new_unbounded(Upperer(&count));

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
    let c = Asyncmemo::with_limits(Upperer(&count), 3, usize::MAX);

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
    let c = Asyncmemo::with_limits(Upperer(&count), 3, 100);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    // Note - this test can fail if "hello" and "HELLO" were allocated differently
    // inside asyncmemo or in the test. If that the case, then fix the test or disable it.
    let expected_weight = String::from("hello").get_weight() + String::from("HELLO").get_weight();
    assert_eq!(c.total_weight(), expected_weight, "c={:#?}", c);
}

#[test]
fn weight_limit_eviction() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::with_limits(Upperer(&count), 1, usize::MAX);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    let expected_weight = String::from("hello").get_weight() + String::from("HELLO").get_weight();
    assert_eq!(c.total_weight(), expected_weight, "c={:#?}", c);

    let v1 = c.get("hell").wait().unwrap();
    assert_eq!(v1, "HELL", "c={:#?}", c);
    assert_eq!(c.len(), 1, "c={:#?}", c);
    let expected_weight = String::from("hell").get_weight() + String::from("HELL").get_weight();
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
    let c = Asyncmemo::new_unbounded(Delayed(&count, 5));

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

struct Fib<'a>(&'a AtomicUsize);

impl<'a> Filler for Fib<'a> {
    type Key = u32;
    type Value = Box<Future<Item = u32, Error = ()> + 'a>;

    fn fill(&self, cache: &Asyncmemo<Self>, key: &u32) -> Self::Value {
        self.0.fetch_add(1, Ordering::Relaxed);

        let key = *key;

        if key == 1 {
            let f = Delay::<u32> {
                remains: 1,
                v: Some(Ok(1)),
            };
            Box::new(f) as Box<Future<Item = u32, Error = ()> + 'a>
        } else {
            let f = cache.get(key - 1).and_then(move |f| Delay {
                remains: 1,
                v: Some(Ok(key + f)),
            });
            Box::new(f) as Box<Future<Item = u32, Error = ()> + 'a>
        }
    }
}

#[test]
fn fibonacci() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded(Fib(&count));

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
        let mut fib = spawn(c.get(4u32));

        let res = fib.poll_future_notify(&notify_handle, dummy_id);
        println!("4: fib.poll()={:?}", res);
        assert_eq!(res, Ok(Async::NotReady));
        assert_eq!(count.load(Ordering::Relaxed), 4);

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
    let c = Asyncmemo::new_unbounded(Fails(&count));

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
    let c = Asyncmemo::new_unbounded(Delayed(&count, 5));

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

#[test]
fn timer_multiwait() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded(Delayed(&count, 2));
    let mut v1 = spawn(c.get("foo"));
    let mut v2 = spawn(c.get("foo"));

    let simple_notify_1 = Arc::new(SimpleNotify::new());
    let notify_handle_1 = NotifyHandle::from(simple_notify_1.clone());
    let dummy_id = 0;
    assert_eq!(
        v1.poll_future_notify(&notify_handle_1, dummy_id),
        Ok(Async::NotReady)
    );

    let simple_notify_2 = Arc::new(SimpleNotify::new());
    let notify_handle_2 = NotifyHandle::from(simple_notify_2.clone());
    assert_eq!(
        v2.poll_future_notify(&notify_handle_2, dummy_id),
        Ok(Async::NotReady)
    );

    assert_eq!(
        v2.poll_future_notify(&notify_handle_2, dummy_id),
        Ok(Async::Ready(String::from("FOO")))
    );

    assert!(*simple_notify_1.was_notified.lock().unwrap());
}
