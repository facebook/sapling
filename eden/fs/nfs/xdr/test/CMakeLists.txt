# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB XDR_TESTS "*.cpp")

add_executable(
  eden_nfs_xdr_test
    ${XDR_TESTS}
)

target_link_libraries(
  eden_nfs_xdr_test
  PUBLIC
    eden_nfs_xdr
    eden_nfs_testharness_xdr_test_utils
    Folly::folly
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_nfs_xdr_test)
