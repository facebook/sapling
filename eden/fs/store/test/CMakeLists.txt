# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_TEST_SRCS "*.cpp")
if (WIN32)
# Few of these tests are complaining while linking to Boost. The link libs
# we have link them against both static and dynamic Boost lib. Right now we
# have only enabled the DiffTest.
  list(
    REMOVE_ITEM STORE_TEST_SRCS
      ${CMAKE_CURRENT_SOURCE_DIR}/BlobAccessTest.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/BlobCacheTest.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/RocksDbLocalStoreTest.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/LocalStoreTest.cpp
      ${CMAKE_CURRENT_SOURCE_DIR}/ObjectStoreTest.cpp
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
