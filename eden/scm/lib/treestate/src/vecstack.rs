/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::mem;

/// Manage a `Vec` of references. so elements with different lifetimes can be
/// pushed manually, and poped automatically. Elements pushed later must have
/// a more narrow lifetime than elements pushed earlier.
///
/// Practically, instead of `vec![&'a T, &'a T, &'a, T, &'a T, ...]`,
/// have something like `vec![&'a T, &'b T, &'c, T, &'d T, ...]`.
/// where `'a` covers `'b`, `'b` covers `'c`....
/// The first function call gets `vec![&'a T]`, the second (nested) one
/// gets `vec![&'b T, &'b T], and so on.
///
/// An example is:
///
/// ```
/// # use treestate::vecstack::VecStack;
/// let mut vec = vec![];
/// let mut stack = VecStack::new(&mut vec);
/// {
///     let str1 = String::from("1");
///     let mut stack = stack.push(&str1);
///     assert_eq!(stack.as_ref()[..], vec!["1"][..]);
///     {
///         // This could also be a recursive function call
///         let str2 = String::from("2");
///         let mut stack = stack.push(&str2);
///         assert_eq!(stack.as_ref()[..], vec!["1", "2"][..]);
///         {
///             let str3 = String::from("3");
///             let stack = stack.push(&str3);
///             assert_eq!(stack.as_ref()[..], vec!["1", "2", "3"][..]);
///         }
///         assert_eq!(stack.as_ref()[..], vec!["1", "2"][..]);
///     }
///     assert_eq!(stack.as_ref()[..], vec!["1"][..]);
/// }
/// assert!(stack.as_ref().is_empty());
/// ```
pub struct VecStack<'a, T: 'a>
where
    T: ?Sized,
{
    // Multiple &mut is undefined behavior.
    // Use a pointer to disable optimization.
    inner: *mut Vec<&'a T>,

    // `inner.len()` required to be able to do `push`
    prepush_len: usize,

    // `inner.len()` after `drop`
    postdrop_len: usize,
}

impl<'a, T: ?Sized> Drop for VecStack<'a, T> {
    fn drop(&mut self) {
        unsafe { self.inner.as_mut().unwrap() }.truncate(self.postdrop_len);
    }
}

impl<'outer, T: ?Sized> VecStack<'outer, T> {
    /// Construct a `VecStack` from a `Vec`. Once constructed, the `Vec`
    /// becomes fully managed by this `VecStack` until this `VecStack` is
    /// dropped. Use `as_ref` for reading, `push` for writing.
    pub fn new(vec: &'outer mut Vec<&'outer T>) -> VecStack<'outer, T> {
        let len = vec.len();
        VecStack {
            inner: vec,
            prepush_len: len,
            postdrop_len: len,
        }
    }

    /// Push a reference with a narrowed lifetime. Return a new `VecStack`.
    ///
    /// Panic if `push` is called when a previously pushed element is
    /// not popped (ex. the result of a previous `push` is not dropped).
    ///
    /// ```should_panic
    /// # use treestate::vecstack::VecStack;
    /// let mut vec = vec![];
    /// let mut stack0 = VecStack::new(&mut vec);
    /// let str1 = String::from("1");
    /// let mut stack1 = stack0.push(&str1);
    /// let str2 = String::from("2");
    /// let mut stack2 = stack0.push(&str2); // panic since `stack1` is alive
    /// ```
    pub fn push<'inner>(&mut self, elem: &'inner T) -> VecStack<'inner, T>
    where
        'outer: 'inner,
    {
        // This casts `vec` from `Vec<&'outer T>` to `Vec<&'inner T>`. This is safe because:
        // - `'inner` is a narrow lifetime. Guaranteed by the where clause. So reading `'outer`
        //   using `'inner` lifetime is fine.
        // - As long as the new `VecStack` is alive, `'inner` is valid. Guaranteed by the lifetime
        //   checker.
        // - Once `VecStack` is dead, `'inner` reference will be removed from `vec`. Guaranteed by
        //   `Drop`.
        // - As long as the new `VecStack` is alive, reading (`as_ref`) via `'outer` is forbidden.
        //   Guaranteed by the assert check in `as_ref`.
        // - As long as the new `VecStack` is alive, writing (`push`) via `'outer` is forbidden.
        //   Guaranteed by the assert check here.
        assert_eq!(
            unsafe { self.inner.as_ref().unwrap() }.len(),
            self.prepush_len,
            "cannot push if vec is changed"
        );
        let vec: &'inner mut Vec<&'inner T> =
            unsafe { mem::transmute(self.inner as *mut Vec<&'outer T>) };
        let postdrop_len = vec.len();
        vec.push(elem);
        let prepush_len = vec.len();
        VecStack {
            inner: vec,
            prepush_len,
            postdrop_len,
        }
    }
}

impl<'a, T: ?Sized> AsRef<Vec<&'a T>> for VecStack<'a, T> {
    /// Get a read-only reference of the actual `Vec`.
    ///
    /// When a reference returned by `as_ref` is alive, it's not allowed
    /// to push again. This is checked statically.
    ///
    /// ```compile_fail
    /// # use treestate::vecstack::VecStack;
    /// let mut vec = vec![];
    /// let mut stack = VecStack::new(&mut vec);
    /// let vecref = stack.as_ref();
    /// let str1 = String::from("1");
    /// let _ = stack.push(&str1); // cannot push when `vecref` is alive.
    /// let __ = vecref;
    /// ```
    ///
    /// Panic if `as_ref` is called when a previously pushed element is
    /// not poped (ex. the result of a previous `push` is not dropped).
    ///
    /// ```should_panic
    /// # use treestate::vecstack::VecStack;
    /// let mut vec = vec![];
    /// let mut stack0 = VecStack::new(&mut vec);
    /// let str1 = String::from("1");
    /// let mut stack1 = stack0.push(&str1);
    /// let ref0 = stack0.as_ref(); // panic since `stack1` is alive
    /// ```
    fn as_ref(&self) -> &Vec<&'a T> {
        let result = unsafe { self.inner.as_ref().unwrap() };
        assert_eq!(
            result.len(),
            self.prepush_len,
            "cannot get reference if vec is changed"
        );
        result
    }
}
