/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Serialize;
use thiserror::Error;

use crate::wire::{ToWire, WireError};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[derive(Serialize)] // used to convert to Python
#[error("server error: {message}")]
pub struct ServerError {
    message: String,
}

impl ServerError {
    pub fn new<M: Into<String>>(m: M) -> Self {
        Self { message: m.into() }
    }
}

impl From<anyhow::Error> for ServerError {
    fn from(e: anyhow::Error) -> Self {
        Self::new(format!("{:?}", e))
    }
}

impl ToWire for ServerError {
    type Wire = WireError;

    fn to_wire(self) -> Self::Wire {
        WireError::new(self.message)
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for ServerError {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        ServerError::new(String::arbitrary(g))
    }
}
