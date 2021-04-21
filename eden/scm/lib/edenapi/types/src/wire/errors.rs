/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::{Deserialize, Serialize};

use crate::wire::{ToApi, ToWire, WireToApiConversionError};
use crate::ServerError;

pub type WireResult<T> = Result<T, WireError>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    #[serde(rename = "1")]
    pub message: Option<String>,
}

impl WireError {
    pub fn new<M: Into<String>>(m: M) -> Self {
        Self {
            message: Some(m.into()),
        }
    }
}

impl ToApi for WireError {
    type Api = ServerError;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        let message = self
            .message
            .ok_or_else(|| WireToApiConversionError::CannotPopulateRequiredField("message"))?;
        Ok(ServerError::new(message))
    }
}

impl<T, E> ToWire for Result<T, E>
where
    T: ToWire,
    E: ToWire,
{
    type Wire = Result<<T as ToWire>::Wire, <E as ToWire>::Wire>;

    fn to_wire(self) -> Self::Wire {
        self.map(|x| x.to_wire()).map_err(|e| e.to_wire())
    }
}

impl<T, E> ToApi for Result<T, E>
where
    T: ToApi,
    E: ToApi,
{
    type Api = Result<<T as ToApi>::Api, <E as ToApi>::Api>;
    type Error = WireToApiConversionError;

    fn to_api(self) -> Result<Self::Api, Self::Error> {
        match self {
            Ok(x) => match x.to_api() {
                Ok(y) => Ok(Ok(y)),
                Err(te) => Err(te.into()),
            },
            Err(e) => match e.to_api() {
                Ok(ae) => Ok(Err(ae)),
                Err(ee) => Err(ee.into()),
            },
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl quickcheck::Arbitrary for WireError {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        WireError::new(String::arbitrary(g))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    use crate::wire::tests::{check_serialize_roundtrip, check_wire_roundtrip};

    quickcheck! {
        fn test_serialize_roundtrip_error(v: WireError) -> bool {
            check_serialize_roundtrip(v)
        }

        fn test_wire_roundtrip_error(v: ServerError) -> bool {
            check_wire_roundtrip(v)
        }
    }
}
