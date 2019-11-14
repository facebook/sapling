/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `stackdesc` provides utilities to describe what the current thread is
//! doing.
//!
//! Generating description has extra cost. So `stackdesc` API makes them lazy.
//! If nobody asks for the stack description, then the overhead is minimal.
//!
//! Typical usecases are:
//! - Adding more context on error.
//! - Understanding why an operation happens. For example, why a network
//!   side-effect was triggered.
//!
//! Example:
//!
//! ```
//! # use stackdesc::{describe, render_stack};
//! # use std::cell::Cell;
//!
//! fn fetch_rev(rev: &str) {
//!     describe!("fetch(rev={})", rev);
//!     fetch_files(&["a", "b"]);
//! }
//!
//! fn fetch_files(items: &[&str]) {
//!     describe!("fetch_files(len={})", items.len());
//!
//!     // For testing
//!     let rendered = render_stack();
//!     assert_eq!(rendered, vec!["fetch(rev=master)", "fetch_files(len=2)"]);
//! }
//!
//! fetch_rev("master");
//! ```

use std::cell::RefCell;
use std::pin::Pin;

/// Contains logic to render description for a scope.
///
/// [`ScopeDescription`] expects first-create-first-drop. The easiest way to
/// achieve that is to use the [`describe!`] macro without using
/// [`ScopeDescription`] directly.
///
/// [`ScopeDescription`] usually matches a stack frame. That is, it is usually
/// put at the top of a function body and describes what that function does.
pub struct ScopeDescription<'a> {
    /// A function that returns meaningful description of the current frame.
    describe_func: Box<dyn Fn() -> String + 'a>,
}

thread_local! {
    static STACK: RefCell<Vec<&'static ScopeDescription<'static>>> = RefCell::new(Vec::new());
}

impl<'a> ScopeDescription<'a> {
    pub fn new(describe_func: impl Fn() -> String + 'a) -> Pin<Box<ScopeDescription<'a>>> {
        let frame = Box::pin(ScopeDescription {
            describe_func: Box::new(describe_func),
        });
        STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let frame: &ScopeDescription<'a> = &frame;
            // This is safe because ScopeDescription::drop removes its reference from the
            // thread local state.
            let frame: &'static ScopeDescription<'static> = unsafe { std::mem::transmute(frame) };
            stack.push(frame)
        });
        frame
    }

    pub fn render(&self) -> String {
        (self.describe_func)()
    }
}

impl<'a> Drop for ScopeDescription<'a> {
    fn drop(&mut self) {
        STACK.with(|stack| {
            let mut stack = stack.borrow_mut();
            let frame = stack.pop().unwrap();
            assert_eq!(
                frame as *const ScopeDescription, self as *const ScopeDescription,
                "incorrect use of ScopeDescription: not dropped in order"
            );
        });
    }
}

/// Render descriptions for the current stack.
/// Return strings in this order: outer first, inner last.
pub fn render_stack() -> Vec<String> {
    STACK.with(|stack| {
        let stack = stack.borrow();
        stack.iter().map(|f| f.render()).collect()
    })
}

/// A shortcut to `let _desc = ScopeDescription::new(|| format!(...))`.
#[macro_export]
macro_rules! describe {
    ($($arg:tt)*) => {
        let _frame = $crate::ScopeDescription::new(|| format!($($arg)*));
    };
}
