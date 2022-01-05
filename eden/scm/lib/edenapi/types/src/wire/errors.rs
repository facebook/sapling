/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::wire::ToApi;
use crate::wire::ToWire;
use crate::wire::WireToApiConversionError;
use crate::ServerError;

pub type WireResult<T> = Result<T, WireError>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WireError {
    #[serde(rename = "1")]
    pub message: Option<String>,
    #[serde(rename = "2")]
    pub code: Option<u64>,
}

impl WireError {
    pub fn new<M: Into<String>>(m: M, code: u64) -> Self {
        Self {
            message: Some(m.into()),
            code: Some(code),
        }
    }
}

impl ToWire for ServerError {
    type Wire = WireError;

    fn to_wire(self) -> Self::Wire {
        WireError {
            message: Some(self.message),
            code: Some(self.code),
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
        let code = self.code.unwrap_or(0);
        Ok(ServerError::new(message, code))
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
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        WireError::new(String::arbitrary(g), u64::arbitrary(g))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::tests::auto_wire_tests;

    auto_wire_tests!(WireError);
}
