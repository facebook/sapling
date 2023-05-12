# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_executable(
  eden_fuse_privhelper_test
    PrivHelperTest.cpp
    PrivHelperTestServer.cpp
)
target_link_libraries(
  eden_fuse_privhelper_test
  PUBLIC
    eden_fuse_privhelper
    eden_utils_test_lib
    eden_testharness
    Folly::folly_test_util
    ${LIBGMOCK_LIBRARIES}
)
gtest_discover_tests(eden_fuse_privhelper_test)

add_executable(drop_privs DropPrivs.cpp)
target_link_libraries(drop_privs eden_fuse_privhelper Folly::folly)
