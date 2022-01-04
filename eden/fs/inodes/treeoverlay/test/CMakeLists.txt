# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


add_executable(
  eden_tree_overlay_test
    TreeOverlayStoreTest.cpp
)

target_link_libraries(
  eden_tree_overlay_test
  PRIVATE
    eden_inodes
    eden_overlay_thrift_cpp
    eden_tree_overlay
    eden_model
    eden_sqlite
    eden_utils
    Folly::folly
    ${LIBGMOCK_LIBRARIES}
)

gtest_discover_tests(eden_tree_overlay_test)
