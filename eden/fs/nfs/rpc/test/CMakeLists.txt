# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB RPC_TESTS "*.cpp")

add_executable(
  eden_nfs_rpc_test
    ${RPC_TESTS}
)

target_link_libraries(
  eden_nfs_rpc_test
  PUBLIC
    eden_nfs_rpc
    eden_nfs_testharness_xdr_test_utils
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_nfs_rpc_test)
