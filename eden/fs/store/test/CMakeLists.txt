# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_TEST_SRCS "*.cpp")
if (WIN32)
  # TODO: T66530055
  # Adding the RocksDb test causes the test binary to not load properly.
  # The LocalStore test fails to compile on Windows.
  list(
    REMOVE_ITEM STORE_TEST_SRCS
      ${CMAKE_CURRENT_SOURCE_DIR}/RocksDbLocalStoreTest.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/LocalStoreTest.cpp
  )
endif()

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
