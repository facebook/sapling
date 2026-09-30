/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Boxed values with an allocation-free shared default.

use std::convert::Infallible;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::ops::Deref;

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;

use crate::adapter::ThriftAdapter;
use crate::metadata::ThriftAnnotations;

/// Supplies a shared value equal to `Self::default()` without allocating.
pub trait DefaultRef: Default {
    /// The reference need only live as long as the caller requires and `Self` permits.
    fn default_ref<'a>() -> &'a Self
    where
        Self: 'a;
}

/// Stores default values without a box and serializes as the underlying `T`.
#[derive(Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct DefaultBox<T>(Option<Box<T>>);

impl<T> Default for DefaultBox<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T> DefaultBox<T> {
    /// Returns the stored value, or `None` when represented by the shared default.
    pub fn as_ref(&self) -> Option<&T> {
        self.0.as_deref()
    }
}

impl<T: DefaultRef + PartialEq> From<T> for DefaultBox<T> {
    fn from(value: T) -> Self {
        Self((value != *T::default_ref()).then(|| Box::new(value)))
    }
}

impl<T: DefaultRef> Deref for DefaultBox<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref().unwrap_or_else(T::default_ref)
    }
}

impl<T: DefaultRef + Serialize> Serialize for DefaultBox<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.deref().serialize(serializer)
    }
}

impl<'de, T: DefaultRef + PartialEq + Deserialize<'de>> Deserialize<'de> for DefaultBox<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        T::deserialize(deserializer).map(Self::from)
    }
}

/// Adapts a Thrift value to [`DefaultBox`], borrowing it during field serialization.
pub struct DefaultBoxAdapter<T>(PhantomData<T>);

impl<T> ThriftAdapter for DefaultBoxAdapter<T>
where
    T: DefaultRef + Clone + Debug + PartialEq + Send + Sync,
{
    type StandardType = T;
    type AdaptedType = DefaultBox<T>;
    type Error = Infallible;

    fn from_thrift(value: T) -> Result<Self::AdaptedType, Self::Error> {
        Ok(value.into())
    }

    fn to_thrift(value: &Self::AdaptedType) -> T {
        (**value).clone()
    }

    fn with_thrift_field<S: ThriftAnnotations, R>(
        value: &Self::AdaptedType,
        _field_id: i16,
        f: impl FnOnce(&T) -> R,
    ) -> R {
        f(value)
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;
    use crate::metadata::NoThriftAnnotations;

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Borrowed<'a>(&'a str);

    impl DefaultRef for Borrowed<'_> {
        fn default_ref<'a>() -> &'a Self
        where
            Self: 'a,
        {
            &Self("")
        }
    }

    #[test]
    fn non_static_values_and_defaults() {
        fn check<'a>(text: &'a str) {
            let empty = DefaultBox::<Borrowed<'a>>::default();
            assert!(empty.as_ref().is_none());
            assert_eq!(*empty, Borrowed(""));
            assert!(DefaultBox::from(Borrowed("")).as_ref().is_none());

            let value = DefaultBox::from(Borrowed(text));
            assert_eq!(*value, Borrowed(text));
            DefaultBoxAdapter::with_thrift_field::<NoThriftAnnotations, _>(&value, 1, |borrowed| {
                assert!(ptr::eq(borrowed, value.as_ref().unwrap()));
            });
        }

        check(&String::from("borrowed"));
    }

    #[test]
    fn serde_preserves_borrowing_and_default_representation() {
        for text in ["", "borrowed"] {
            let json = serde_json::to_string(text).unwrap();
            let value: DefaultBox<Borrowed<'_>> = serde_json::from_str(&json).unwrap();
            assert_eq!(*value, Borrowed(text));
            assert_eq!(value.as_ref().is_none(), text.is_empty());
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
        }
    }
}
