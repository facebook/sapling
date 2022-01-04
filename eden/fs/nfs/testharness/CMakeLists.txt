# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_nfs_testharness_xdr_test_utils STATIC
    XdrTestUtils.cpp XdrTestUtils.h
)

target_link_libraries(
  eden_nfs_testharness_xdr_test_utils
  PUBLIC
    Folly::folly
)
