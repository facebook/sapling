/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

/// Define a primitive or derived atom.
#[macro_export]
macro_rules! atom {
    // Primitive atom. Has interior mutability.
    ($name:ident, RwLock<$value:ty>) => {
        $crate::atom!($name, mut RwLock<$value>);
    };
    ($name:ident, $mod:ident :: RwLock<$value:ty>) => {
        $crate::atom!($name, mut $mod :: RwLock<$value>);
    };
    ($name:ident, Mutex<$value:ty>) => {
        $crate::atom!($name, mut Mutex<$value>);
    };
    ($name:ident, mut $value:ty) => {
        pub struct $name;
        impl $crate::Atom for $name {
            type Value = $value;
            fn has_interior_mutability() -> bool {
                true
            }
        }
    };

    // Primitive atom.
    ($name:ident, $value:ty) => {
        pub struct $name;
        impl $crate::Atom for $name {
            type Value = $value;
        }
    };

    // Derived atom. `value` impls `PartialEq`. Avoids unnecessary updates.
    ($name:ident, $value:ty, |$store:ident| $body:expr) => {
        $crate::atom!($name, $value, |$store, prev| {
            let value: $crate::Result<::std::sync::Arc<Self::Value>> = $body;
            let value = value?;
            if let Some(prev) = prev {
                if prev.as_ref() == value.as_ref() {
                    return Ok(prev);
                }
            }
            Ok(value)
        });
    };

    // Derived atom. Customized `prev` logic.
    ($name:ident, $value:ty, |$store:ident, $prev:ident| $body:expr) => {
        pub struct $name;
        impl $crate::Atom for $name {
            type Value = $value;
            fn calculate(
                #[allow(unused)] $store: &impl $crate::GetAtomValue,
                $prev: Option<::std::sync::Arc<Self::Value>>,
            ) -> $crate::Result<::std::sync::Arc<Self::Value>> {
                $body
            }
        }
    };

    // Primitive atom with initial value.
    ($name:ident, $value:ty, $initial:expr) => {
        $crate::atom!($name, $value, |_store, _prev| Ok(::std::sync::Arc::new($initial)));
    };
}
