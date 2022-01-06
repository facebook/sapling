/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub use self::ffi::create_metadata;

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("eden/mononoke/scs_server/metadata.h");

        #[namespace = "facebook::rust::srserver"]
        type RustThriftMetadata = srserver::RustThriftMetadata;

        #[namespace = "facebook::scm::service"]
        fn create_metadata() -> UniquePtr<RustThriftMetadata>;
    }
}
