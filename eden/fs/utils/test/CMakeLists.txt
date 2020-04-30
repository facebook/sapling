# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB UTILS_TEST_SRCS "*Test.cpp")
if (WIN32)
  list(
    REMOVE_ITEM UTILS_TEST_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/CoverageSetTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/IoFutureTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/MappedDiskVectorTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcessAccessLogTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcessNameCacheTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcUtilTest.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/UnixSocketTest.cpp
  )
endif()

add_executable(eden_utils_test ${UTILS_TEST_SRCS})
target_link_libraries(
  eden_utils_test
  PUBLIC
    eden_utils_test_lib
    eden_testharness
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)
gtest_discover_tests(eden_utils_test)

add_library(
  eden_utils_test_lib STATIC
    ScopedEnvVar.h
    ScopedEnvVar.cpp
)
target_link_libraries(
  eden_utils_test_lib
    Folly::folly
)
