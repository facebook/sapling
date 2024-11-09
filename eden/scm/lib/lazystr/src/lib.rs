/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

pub trait LazyStr {
    fn to_str<'a>(self) -> Cow<'a, str>;
}

impl<F: FnOnce() -> String> LazyStr for F {
    fn to_str<'a>(self) -> Cow<'a, str> {
        self().into()
    }
}

impl LazyStr for &'static str {
    fn to_str<'a>(self) -> Cow<'a, str> {
        self.into()
    }
}

impl LazyStr for String {
    fn to_str<'a>(self) -> Cow<'a, str> {
        self.into()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn use_lazy<'a>(l: impl LazyStr) -> Cow<'a, str> {
        l.to_str()
    }

    fn ignore_lazy(l: impl LazyStr) {
        let _ = l;
    }

    #[test]
    fn test_lazy_str() {
        assert_eq!(use_lazy("foo"), "foo");
        assert_eq!(use_lazy("bar".to_string()), "bar");
        assert_eq!(use_lazy(|| "baz".to_string()), "baz");

        // sanity check laziness works
        ignore_lazy(|| panic!("oops"));
    }
}
