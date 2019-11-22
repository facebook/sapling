# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  datapack
  STATIC
    cstore/datapackstore.cpp
    cstore/deltachain.cpp
    cstore/uniondatapackstore.cpp
    ctreemanifest/manifest.cpp
    ctreemanifest/manifest_entry.cpp
    ctreemanifest/manifest_fetcher.cpp
    ctreemanifest/manifest_ptr.cpp
    ctreemanifest/treemanifest.cpp
)
target_link_libraries(
  datapack
  PUBLIC
    eden_buffer
    eden_mpatch
    eden_cdatapack
)
