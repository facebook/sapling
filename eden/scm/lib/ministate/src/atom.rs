/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::any::type_name;
use std::sync::Arc;

use anyhow::Result;
use anyhow::bail;

/// A `Atom` defines the value type, and how to calculate the value.
pub trait Atom: 'static {
    type Value: Any + Send + Sync + 'static;

    /// Calculate `Value`. Use `store.get` to get dependent values.
    ///
    /// If any of the dependent values are changed, this `Value` will
    /// be re-calculated on the next `store.get` read.
    ///
    /// Returning `prev_value` as-is indicates the value is not changed,
    /// despite dependencies might have changed. This helps reduce
    /// unnecessary re-calculation.
    fn calculate(
        _store: &impl GetAtomValue,
        _prev_value: Option<Arc<Self::Value>>,
    ) -> Result<Arc<Self::Value>> {
        bail!("{} cannot be calculated", type_name::<Self>());
    }

    /// Should report `true` if `Value` has interior mutability.
    /// Affects `store.set` behavior.
    fn has_interior_mutability() -> bool {
        false
    }
}

/// A `PrimitiveValue` does not have a separate `Atom` type.
pub trait PrimitiveValue {}

impl<T: PrimitiveValue + Send + Sync + 'static> Atom for T {
    type Value = T;
}

/// A `store` that can resolve an atom to its value.
pub trait GetAtomValue {
    /// Get the value of an `Atom`. Calculate on demand.
    fn get<T: Atom>(&self) -> Result<Arc<T::Value>>;
}
