# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_TEST_SRCS "*Test.cpp")
list(REMOVE_ITEM STORE_TEST_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/FilteredBackingStoreTest.cpp)
add_executable(
  eden_store_test
  ${STORE_TEST_SRCS}
)
target_link_libraries(
  eden_store_test
  PUBLIC
    eden_store
    eden_model
    eden_telemetry
    eden_testharness
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)
gtest_discover_tests(eden_store_test NO_PRETTY_VALUES)
