/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use stack_config_derive::*;

pub mod __private {
    pub use serde_derive::Deserialize;

    pub trait ConfigLayerType {
        type Layer;
    }

    impl<T: ConfigLayerType> ConfigLayerType for Option<T> {
        type Layer = Option<<T as ConfigLayerType>::Layer>;
    }

    pub trait ConfigLayer: Default {
        type Product;

        fn finalize(self) -> Result<Self::Product, String>;
        fn merge(&mut self, other: Self);
    }

    impl<T: ConfigLayer> ConfigLayer for Option<T> {
        type Product = Option<<T as ConfigLayer>::Product>;

        fn finalize(self) -> Result<Self::Product, String> {
            match self {
                Some(inner) => inner.finalize().map(Some),
                None => Ok(None),
            }
        }

        fn merge(&mut self, other: Self) {
            match (self.as_mut(), other) {
                (Some(lhs), Some(rhs)) => lhs.merge(rhs),
                // if we don't have base layer yet, we need to initialize it with the default value.
                (None, Some(rhs)) => {
                    let mut lhs = <T as Default>::default();
                    lhs.merge(rhs);
                    *self = Some(lhs);
                }
                (_, None) => (),
            }
        }
    }
}
