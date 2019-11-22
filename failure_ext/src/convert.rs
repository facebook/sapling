/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::Error;
use failure::Fail;
use std::error::Error as StdError;
use std::fmt::{self, Debug, Display};

pub fn convert(fail: impl Fail) -> Error {
    convert_ref(&fail)
}

fn convert_ref(fail: &(impl Fail + ?Sized)) -> Error {
    match fail.cause() {
        Some(cause) => convert_ref(cause).context(fail.to_string()),
        None => Error::new(ErrorMessage {
            display: fail.to_string(),
            debug: format!("{:?}", fail),
        }),
    }
}

struct ErrorMessage {
    display: String,
    debug: String,
}

impl StdError for ErrorMessage {}

impl Display for ErrorMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&self.display)
    }
}

impl Debug for ErrorMessage {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str(&self.debug)
    }
}
