/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

#![deny(warnings, missing_docs, clippy::all, rustdoc::broken_intra_doc_links)]

//! See examples for what code you can write with cloned macro.
//!
//! # Examples
//!
//! ```
//! # use cloned::cloned;
//! struct A {
//!     x: String,
//!     y: String,
//!     z: String,
//! }
//! impl A {
//!     fn foo(&self) {
//!         cloned!(self.x, self.y, self.z);
//!         (move || {
//!             println!("{} {} {}", x, y, z);
//!         })();
//!     }
//! }
//! # fn main () {}
//! ```
//!
//! It also supports setting a local alias:
//! ```
//! # use cloned::cloned;
//! # fn main () {
//! let foo = 42;
//! cloned!(foo as bar);
//! assert!(foo == bar);
//! # }
//! ```

/// See crate's documentation
#[macro_export]
macro_rules! cloned {
    ($i:ident as $alias:ident) => {
        let $alias = $i.clone();
    };
    (mut $i:ident as $alias:ident) => {
        let mut $alias = $i.clone();
    };
    ($i:ident as $alias:ident, $($tt:tt)*) => {
        cloned!($i as $alias);
        cloned!($($tt)*);
    };
    (mut $i:ident as $alias:ident, $($tt:tt)*) => {
        cloned!(mut $i as $alias);
        cloned!($($tt)*);
    };
    ($this:ident . $i:ident as $alias:ident) => {
        let $alias = $this.$i.clone();
    };
    (mut $this:ident . $i:ident as $alias:ident) => {
        let mut $alias = $this.$i.clone();
    };
    ($this:ident . $i:ident as $alias:ident, $($tt:tt)*) => {
        cloned!($this . $i as $alias);
        cloned!($($tt)*);
    };
    (mut $this:ident . $i:ident as $alias:ident, $($tt:tt)*) => {
        cloned!(mut $this . $i as $alias);
        cloned!($($tt)*);
    };

    ($i:ident) => {
        cloned!($i as $i)
    };
    (mut $i:ident) => {
        cloned!(mut $i as $i)
    };
    ($i:ident, $($tt:tt)*) => {
        cloned!($i as $i);
        cloned!($($tt)*);
    };
    (mut $i:ident, $($tt:tt)*) => {
        cloned!(mut $i);
        cloned!($($tt)*);
    };

    ($this:ident . $i:ident) => {
        cloned!($this.$i as $i)
    };
    (mut $this:ident . $i:ident) => {
        let mut $i = $this.$i.clone();
    };
    ($this:ident . $i:ident, $($tt:tt)*) => {
        cloned!($this . $i as $i);
        cloned!($($tt)*);
    };
    (mut $this:ident . $i:ident, $($tt:tt)*) => {
        cloned!(mut $this . $i);
        cloned!($($tt)*);
    };

    // Handle trailing ','
    () => {};
}

#[cfg(test)]
mod tests {
    struct A {
        x: String,
    }

    impl A {
        #[allow(clippy::let_and_return)]
        fn foo(&self) -> String {
            cloned!(self.x);
            x
        }
    }

    #[test]
    fn test() {
        let a = A {
            x: "I am a struct".into(),
        };
        let y: String = "that can".into();
        let z: String = "talk a lot".into();
        {
            cloned!(a.x, y, mut z);
            let _ = a.foo();
            assert_eq!(&format!("{x} {y} {z}"), "I am a struct that can talk a lot");
            z = String::new();
            assert_eq!(z, "");
        }
    }

    #[test]
    #[allow(unused_variables, unused_assignments)]
    fn test_mut() {
        let a = 1;
        let b = 2;
        let c = A {
            x: "foo".to_string(),
        };

        cloned!(mut a);
        a += 1;
        cloned!(mut a, b);
        a += 1;
        cloned!(a, mut b);
        b += 1;
        cloned!(mut c.x);
        x += "bar";
        cloned!(c.x, mut a);
        a += 1;
        cloned!(a, mut c.x);
        x += "bar";
    }

    #[test]
    fn trailing_comma() {
        let a = 1;
        let b = 2;

        cloned!(a, b,);

        assert_eq!((a, b), (1, 2))
    }

    #[test]
    fn trailing_comma_mut() {
        let a = 1;
        let b = 2;

        cloned!(a, mut b,);

        b += 2;

        assert_eq!((a, b), (1, 4))
    }

    #[test]
    #[allow(unused_variables, unused_mut)]
    fn aliases() {
        let a = 1;
        let b = 2;
        let c = A {
            x: "foo".to_string(),
        };

        cloned!(a as a2);
        cloned!(a as a2,);
        cloned!(mut a as a2);
        cloned!(mut a as a2,);
        cloned!(c.x as x2);
        cloned!(c.x as x2,);
        cloned!(mut c.x as x2);
        cloned!(mut c.x as x2,);

        cloned!(a, a as a2);
        cloned!(a, a as a2,);
        cloned!(a, mut a as a2);
        cloned!(a, mut a as a2,);
        cloned!(a, c.x as x2);
        cloned!(a, c.x as x2,);
        cloned!(a, mut c.x as x2);
        cloned!(a, mut c.x as x2,);
    }
}
