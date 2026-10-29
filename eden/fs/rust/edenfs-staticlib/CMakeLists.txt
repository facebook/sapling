# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if (IS_FB_BUILD)
  set(features FEATURES fb)
endif()
rust_static_library(
  rust_edenfs_staticlib
  CRATE edenfs_staticlib
  PACKAGE edenfs-staticlib
  USE_CXX_INCLUDE
  ${features}
  ENV ${EDEN_RUST_THRIFT_ENV}
  # The blake3 crate assembles BLAKE3's SIMD code under the same symbol names as
  # the C library that eden's C++ links, and defines more of them when it's the
  # newer of the two. With the C library after the Rust one, the linker never
  # takes both copies of a symbol.
  # The curl crate needs libcurl from the final link, except on Windows, where
  # it builds its own copy.
  LINK_LIBRARIES backingstore_iobuf BLAKE3::blake3
    $<TARGET_NAME_IF_EXISTS:CURL::libcurl>)

install_rust_static_library(
  rust_edenfs_staticlib
  EXPORT mercurial
  INSTALL_DIR lib
)
