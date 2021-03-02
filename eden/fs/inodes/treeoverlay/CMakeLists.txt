# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TREE_OVERLAY_SRCS "*.cpp")

add_library(
  eden_tree_overlay STATIC
    ${TREE_OVERLAY_SRCS}
)

target_link_libraries(
  eden_tree_overlay
  PRIVATE
    eden_inodes
    eden_sqlite
    eden_overlay_thrift_cpp
    eden_utils
    Folly::folly
)

add_subdirectory(test)
