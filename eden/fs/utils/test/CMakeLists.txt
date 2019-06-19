# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB UTILS_TEST_SRCS "*Test.cpp")
add_executable(
  eden_utils_test
  ${UTILS_TEST_SRCS}
)
target_link_libraries(
  eden_utils_test
  PUBLIC
    eden_testharness
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)
gtest_discover_tests(eden_utils_test)
