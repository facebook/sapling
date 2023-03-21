# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB DIGEST_TEST_SRCS "*Test.cpp")
add_executable(
  eden_digest_test
    ${DIGEST_TEST_SRCS}
)

target_link_libraries(
  eden_digest_test
  PRIVATE
    eden_digest
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_digest_test)
