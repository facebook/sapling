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
  ENV ${EDEN_RUST_THRIFT_ENV})

install_rust_static_library(
  rust_edenfs_staticlib
  EXPORT mercurial
  INSTALL_DIR lib
)
