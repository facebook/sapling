/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cell::RefCell;
use std::sync::Arc;

use crate::PrimitiveValue;
use crate::Store;
use crate::atom;

#[test]
fn test_primitive_atom() {
    atom!(S, u32);

    let store = Store::new();
    let err = store.get::<S>().unwrap_err().to_string();
    assert!(err.ends_with("S cannot be calculated"));

    store.set::<S>(Arc::new(12));
    assert_eq!(*store.get::<S>().unwrap(), 12);

    store.set::<S>(Arc::new(123));
    assert_eq!(*store.get::<S>().unwrap(), 123);
}

#[test]
fn test_primitive_atom_with_initial_value() {
    atom!(S, u32, 21);

    let store = Store::new();
    assert_eq!(*store.get::<S>().unwrap(), 21);

    store.set::<S>(Arc::new(321));
    assert_eq!(*store.get::<S>().unwrap(), 321);
}

#[test]
fn test_primitive_value() {
    struct S(u32);
    impl PrimitiveValue for S {}

    let store = Store::new();
    store.set::<S>(Arc::new(S(12)));
    assert_eq!(store.get::<S>().unwrap().0, 12);
}

atom!(A, u32);
atom!(B, u32);
atom!(C, u32);

thread_local! {
    static RECALC: RefCell<String> = const { RefCell::new(String::new()) };
}
fn track_recalc(f: impl FnOnce()) -> String {
    RECALC.with(|recalc| {
        recalc.borrow_mut().clear();
    });
    f();
    RECALC.with(|recalc| recalc.borrow().clone())
}
fn mark_recalc(name: char) {
    RECALC.with(|recalc| {
        recalc.borrow_mut().push(name);
    });
}

#[test]
fn test_derived_on_primitive() {
    // S = A + B
    atom!(S, u32, |store| {
        mark_recalc('S');
        let a = store.get::<A>()?;
        let b = store.get::<B>()?;
        Ok(Arc::new(*a + *b))
    });

    // Cannot get S without dependencies.
    let store = Store::new();
    assert!(store.get::<S>().is_err());

    // Calculate from dependencies.
    store.set::<A>(Arc::new(10));
    store.set::<B>(Arc::new(20));
    store.set::<C>(Arc::new(10));
    let recalc = track_recalc(|| assert_eq!(store.get::<S>().unwrap(), Arc::new(30)));
    assert_eq!(recalc, "S");

    // No re-calculation if dependencies are not changed.
    let recalc = track_recalc(|| assert_eq!(store.get::<S>().unwrap(), Arc::new(30)));
    assert_eq!(recalc, "");

    // Changing dependencies triggers re-calculation.
    store.set::<A>(Arc::new(20));
    store.set::<B>(Arc::new(30));
    let recalc = track_recalc(|| assert_eq!(store.get::<S>().unwrap(), Arc::new(50)));
    assert_eq!(recalc, "S");

    // Changing unrelated atom (C) does not trigger re-calculation.
    store.set::<C>(Arc::new(20));
    let recalc = track_recalc(|| assert_eq!(store.get::<S>().unwrap(), Arc::new(50)));
    assert_eq!(recalc, "");
}

#[test]
fn test_derived_on_override() {
    atom!(CountAtom, u32, 1);
    let store = Store::new();
    atom!(DoubledCountAtom, u32, |store| Ok(Arc::new(
        *store.get::<CountAtom>()? * 2
    )));
    assert_eq!(*store.get::<DoubledCountAtom>().unwrap(), 2);

    // Override `CountAtom` triggers `DoubledCountAtom` re-calc.
    store.set::<CountAtom>(2);
    assert_eq!(*store.get::<DoubledCountAtom>().unwrap(), 4);
    store.set::<CountAtom>(3);
    assert_eq!(*store.get::<DoubledCountAtom>().unwrap(), 6);

    // Override `DoubledCountAtom`. `CountAtom` won't trigger `DoubledCountAtom` re-calc.
    store.set::<DoubledCountAtom>(7);
    assert_eq!(*store.get::<DoubledCountAtom>().unwrap(), 7);
    store.set::<CountAtom>(4);
    assert_eq!(*store.get::<DoubledCountAtom>().unwrap(), 7);
}

#[test]
fn test_interior_mutability() {
    use parking_lot::RwLock;
    atom!(M, RwLock<u32>);

    // N = A + M
    atom!(N, u32, |store| {
        mark_recalc('N');
        let a = store.get::<A>()?;
        let m = store.get::<M>()?;
        let m = m.read();
        Ok(Arc::new(*a + *m))
    });

    let store = Store::new();

    // Initial state.
    let a = Arc::new(5);
    let m = Arc::new(RwLock::new(5));
    store.set::<A>(a.clone());
    store.set::<M>(m.clone());
    assert_eq!(store.get::<N>().unwrap(), Arc::new(10));

    // Updating A to the same `Arc` won't trigger recalc of N.
    let recalc = track_recalc(|| {
        store.set::<A>(a);
        assert_eq!(store.get::<N>().unwrap(), Arc::new(10));
    });
    assert_eq!(recalc, "");

    // Updating M to the same `Arc` triggers recalc of N.
    let recalc = track_recalc(|| {
        *m.write() = 6;
        store.set::<M>(m.clone());
        assert_eq!(store.get::<N>().unwrap(), Arc::new(11));
    });
    assert_eq!(recalc, "N");
}

#[test]
fn test_derived_dependency_tree() {
    // P = A / 2
    atom!(P, u32, |store| {
        mark_recalc('P');
        let a = store.get::<A>()?;
        Ok(Arc::new(*a / 2))
    });

    // Q = B / 2
    atom!(Q, u32, |store| {
        mark_recalc('Q');
        let b = store.get::<B>()?;
        Ok(Arc::new(*b / 2))
    });

    // R = max(P, Q)
    atom!(R, u32, |store| {
        mark_recalc('R');
        let p = store.get::<P>()?;
        let q = store.get::<Q>()?;
        Ok(Arc::new(*p.max(q)))
    });

    // X = A + R + C
    atom!(X, u32, |store| {
        mark_recalc('X');
        let a = store.get::<A>()?;
        let r = store.get::<R>()?;
        let c = store.get::<C>()?;
        Ok(Arc::new(*a + *r + *c))
    });

    let store = Store::new();

    // Setting primitive values won't trigger re-calculation.
    let recalc = track_recalc(|| {
        store.set::<A>(Arc::new(10));
        store.set::<B>(Arc::new(20));
        store.set::<C>(Arc::new(30));
    });
    assert_eq!(recalc, "");

    // P won't trigger calculating other values.
    let recalc = track_recalc(|| assert_eq!(store.get::<P>().unwrap(), Arc::new(5)));
    assert_eq!(recalc, "P");

    // R requires (derived) P and Q. P is cached and won't be re-calculated.
    let recalc = track_recalc(|| assert_eq!(store.get::<R>().unwrap(), Arc::new(10)));
    assert_eq!(recalc, "RQ");

    // Changing A without affecting P. R won't be re-calculated.
    let recalc = track_recalc(|| {
        store.set::<A>(Arc::new(11));
        assert_eq!(store.get::<R>().unwrap(), Arc::new(10))
    });
    assert_eq!(recalc, "P");

    // Changing A that changes P. R is re-calculated.
    let recalc = track_recalc(|| {
        store.set::<A>(Arc::new(40));
        assert_eq!(store.get::<R>().unwrap(), Arc::new(20))
    });
    assert_eq!(recalc, "PR");

    // Calculating X.
    let recalc = track_recalc(|| assert_eq!(store.get::<X>().unwrap(), Arc::new(90)));
    assert_eq!(recalc, "X");

    // Change B. Triggers recalc of Q, R but not X (R remains unchanged).
    let recalc = track_recalc(|| {
        store.set::<B>(Arc::new(22));
        assert_eq!(store.get::<X>().unwrap(), Arc::new(90));
    });
    assert_eq!(recalc, "QR");

    // Change B and C. Triggers recalc of Q (bottom layer) and X (top layer),
    // but not R (middle layer, because Q is not changed).
    let recalc = track_recalc(|| {
        store.set::<B>(Arc::new(23));
        store.set::<C>(Arc::new(31));
        assert_eq!(store.get::<X>().unwrap(), Arc::new(91));
    });
    assert_eq!(recalc, "QX");
}

#[test]
fn test_crate_rwlock() {
    atom!(V, crate::RwLock<u32>);

    // W = V + 1
    atom!(W, u32, |store| {
        mark_recalc('W');
        let v = store.get::<V>().unwrap();
        let v = *v.read();
        Ok(Arc::new(v + 1))
    });

    let store = Store::new();
    let v = store.set_rwlock::<V, _>(10);
    assert_eq!(store.get::<W>().unwrap(), Arc::new(11));

    // Reading `v` won't trigger recalc.
    let recalc = track_recalc(|| {
        assert_eq!(*v.read(), 10);
        assert_eq!(store.get::<W>().unwrap(), Arc::new(11));
    });
    assert_eq!(recalc, "");

    // Writing `v` triggers recalc. No need to `store.set::<V>`.
    let recalc = track_recalc(|| {
        *v.write() = 11;
        assert_eq!(store.get::<W>().unwrap(), Arc::new(12));
    });
    assert_eq!(recalc, "W");
}
