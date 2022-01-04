# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_nfs_xdr STATIC
    Xdr.cpp Xdr.h
)

target_link_libraries(
  eden_nfs_xdr
  PUBLIC
    Folly::folly
)

add_subdirectory(test)
