# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TESTHARNESS_SRCS "*.cpp")

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
  eden_fuse
  eden_inodes
  eden_model
  eden_store
  eden_utils
  ${BOOST_LIBRARIES}
  Folly::folly_test_util
  ${LIBGMOCK_LIBRARIES}
  ${GLOG_LIBRARY}
)
