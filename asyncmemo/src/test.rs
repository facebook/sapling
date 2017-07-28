// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::usize;

struct Upperer<'a>(&'a AtomicUsize);
impl<'a> Filler for Upperer<'a> {
    type Key = String;
    type Value = Result<String, ()>;

    fn fill(&self, key: &Self::Key) -> Self::Value {
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

struct Delayed<F>
where
    F: Filler,
{
    inner: F,
    count: usize,
}

struct Delay<F>
where
    F: Future,
{
    fut: F,
    remains: usize,
}

impl<F> Future for Delay<F>
where
    F: Future,
{
    type Item = F::Item;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        if self.remains == 0 {
            self.fut.poll()
        } else {
            self.remains -= 1;
            Ok(Async::NotReady)
        }
    }
}

impl<F> Delayed<F>
where
    F: Filler,
{
    fn new(count: usize, inner: F) -> Self {
        Delayed {
            inner: inner,
            count: count,
        }
    }
}

impl<F> Filler for Delayed<F>
where
    F: Filler,
{
    type Key = F::Key;
    type Value = Delay<<F::Value as IntoFuture>::Future>;

    fn fill(&self, key: &Self::Key) -> Self::Value {
        Delay {
            fut: self.inner.fill(key).into_future(),
            remains: self.count,
        }
    }
}

#[test]
fn more() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded(Delayed::new(2, Upperer(&count)));

    let mut v = c.get("hello");
    assert_eq!(v.poll(), Ok(Async::NotReady));
    assert_eq!(v.poll(), Ok(Async::NotReady));

    match v.poll().unwrap() {
        Async::NotReady => panic!("unexpected not ready"),
        Async::Ready(v) => assert_eq!(v, "HELLO"),
    }
}

#[test]
fn limit() {
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::with_limits(Upperer(&count), 2, usize::MAX);

    assert_eq!(c.len(), 0);

    let v1 = c.get("hello").wait().unwrap();
    assert_eq!(v1, "HELLO");
    assert_eq!(c.len(), 1);

    let v2 = c.get("goodbye").wait().unwrap();
    assert_eq!(v2, "GOODBYE");
    assert_eq!(c.len(), 2);

    let v3 = c.get("world").wait().unwrap();
    assert_eq!(v3, "WORLD");
    assert_eq!(c.len(), 2);

    let v4 = c.get("ungulate").wait().unwrap();
    assert_eq!(v4, "UNGULATE");
    assert_eq!(c.len(), 2);
}

struct FillArced<F>(F);
struct Arcer<F>(F);

impl<F> Filler for FillArced<F>
where
    F: Filler,
{
    type Key = F::Key;
    type Value = Arcer<<F::Value as IntoFuture>::Future>;

    fn fill(&self, k: &F::Key) -> Arcer<<F::Value as IntoFuture>::Future> {
        let f = self.0.fill(k).into_future();
        Arcer(f)
    }
}

impl<F> Future for Arcer<F>
where
    F: Future,
{
    type Item = Arc<F::Item>;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let v = self.0.poll()?;
        Ok(v.map(Arc::new))
    }
}

#[test]
fn expensive() {
    // Use Arc for expensive-to-copy values
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded(FillArced(Upperer(&count)));

    let v = c.get("Hello").wait().unwrap();
    assert_eq!(*v, "HELLO");
}

#[test]
fn invalidate() {
    // Use Arc for expensive-to-copy values
    let count = AtomicUsize::new(0);
    let c = Asyncmemo::new_unbounded(FillArced(Upperer(&count)));

    let v = c.get("Hello").wait().unwrap();
    assert_eq!(*v, "HELLO");
    assert_eq!(count.load(Ordering::Relaxed), 1);

    let v = c.get("Hello").wait().unwrap();
    assert_eq!(*v, "HELLO");
    assert_eq!(count.load(Ordering::Relaxed), 1);

    c.invalidate("Hello");

    let v = c.get("Hello").wait().unwrap();
    assert_eq!(*v, "HELLO");
    assert_eq!(count.load(Ordering::Relaxed), 2);

    let v = c.get("Hello").wait().unwrap();
    assert_eq!(*v, "HELLO");
    assert_eq!(count.load(Ordering::Relaxed), 2);
}
