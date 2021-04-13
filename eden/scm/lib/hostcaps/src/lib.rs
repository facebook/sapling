/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(fbcode_build, feature = "fb"))]
mod facebook;
#[cfg(any(fbcode_build, feature = "fb"))]
pub use facebook::is_prod;

#[cfg(not(any(fbcode_build, feature = "fb")))]
pub fn is_prod() -> bool {
    false
}

#[no_mangle]
pub extern "C" fn eden_is_prod() -> bool {
    is_prod()
}

#[no_mangle]
pub extern "C" fn eden_has_servicerouter() -> bool {
    is_prod()
}
