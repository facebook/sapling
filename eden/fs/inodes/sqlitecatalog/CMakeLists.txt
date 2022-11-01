# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB SQLITE_CATALOG_SRCS "*.cpp")
list(
  REMOVE_ITEM SQLITE_CATALOG_SRCS
  ${CMAKE_CURRENT_SOURCE_DIR}/WindowsOverlayScanner.cpp
)

add_library(
  eden_sqlite_catalog STATIC
    ${SQLITE_CATALOG_SRCS}
)

target_link_libraries(
  eden_sqlite_catalog
  PUBLIC
    eden_sqlite

  PRIVATE
    eden_inodes
    eden_overlay_thrift_cpp
    eden_utils
    Folly::folly
    edencommon::edencommon_utils
)

add_executable(
  eden_scanner
    WindowsOverlayScanner.cpp
)

target_link_libraries(
  eden_scanner
  PRIVATE
    cpptoml
    eden_sqlite_catalog
    eden_utils
    Folly::folly
)

add_subdirectory(test)
