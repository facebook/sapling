# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if(WIN32)
set(
  TESTHARNESS_SRCS
  "FakeBackingStore.cpp"
  "FakeBackingStore.h"
  "FakePrivHelper.cpp"
  "FakePrivHelper.h"
  "FakeTreeBuilder.cpp"
  "FakeTreeBuilder.h"
  "TempFile.cpp"
  "TempFile.h"
  "TestMount.cpp"
  "TestMount.h"
  "TestUtil.cpp"
  "TestUtil.h"
)
else()
file(GLOB TESTHARNESS_SRCS "*.cpp")
endif()

add_library(
  eden_testharness STATIC
  ${TESTHARNESS_SRCS}
)
target_include_directories(eden_testharness
  SYSTEM
  PUBLIC
    ${LIBGMOCK_INCLUDE_DIR}
)
target_link_libraries(
  eden_testharness
  PUBLIC
  eden_config
  eden_inodes
  eden_model
  eden_store
  eden_utils
  edencommon::edencommon_utils
  ${BOOST_LIBRARIES}
  Folly::folly_test_util
  ${LIBGMOCK_LIBRARIES}
  ${GLOG_LIBRARY}
)

if(NOT WIN32)
  target_link_libraries(
    eden_testharness
    PUBLIC
    eden_fuse
  )
endif()
