# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_TEST_SRCS "*.cpp")
add_executable(
  eden_store_test
  ${STORE_TEST_SRCS}
)
target_link_libraries(
  eden_store_test
  PUBLIC
    eden_store
    eden_model
    eden_testharness
    ${LIBGMOCK_LIBRARIES}
)
gtest_discover_tests(eden_store_test)
