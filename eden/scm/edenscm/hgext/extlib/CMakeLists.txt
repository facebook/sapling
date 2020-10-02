# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  datapack
  STATIC
    ctreemanifest/manifest.cpp
    ctreemanifest/manifest_entry.cpp
    ctreemanifest/manifest_fetcher.cpp
    ctreemanifest/manifest_ptr.cpp
)

target_link_libraries(
  datapack
  PUBLIC
    OpenSSL::Crypto
)

target_include_directories(
  datapack
  PUBLIC
    "${CMAKE_SOURCE_DIR}/eden/scm"
)
