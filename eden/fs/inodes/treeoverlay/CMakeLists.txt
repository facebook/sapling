# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TREE_OVERLAY_SRCS "*.cpp")
list(
  REMOVE_ITEM TREE_OVERLAY_SRCS
  ${CMAKE_CURRENT_SOURCE_DIR}/TreeOverlayScanner.cpp
)

add_library(
  eden_tree_overlay STATIC
    ${TREE_OVERLAY_SRCS}
)

target_link_libraries(
  eden_tree_overlay
  PUBLIC
    eden_sqlite

  PRIVATE
    eden_inodes
    eden_overlay_thrift_cpp
    eden_utils
    Folly::folly
    edencommon::utils
)

add_executable(
  eden_scanner
    TreeOverlayScanner.cpp
)

target_link_libraries(
  eden_scanner
  PRIVATE
    cpptoml
    eden_tree_overlay
    eden_utils
    Folly::folly
)

add_subdirectory(test)
