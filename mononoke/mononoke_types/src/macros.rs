/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

// Extract optional Thrift field into a Result for easy ?
#[macro_export]
macro_rules! thrift_field {
    ($struct:ident, $thrift:ident, $field:ident) => {
        $thrift
            .$field
            .ok_or($crate::errors::ErrorKind::InvalidThrift(
                stringify!($struct).into(),
                format!("Missing field: {}", stringify!($field)),
            ))
    };
}
