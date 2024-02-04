# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB LMDB_SRCS "*.cpp")
add_library(
  eden_lmdb STATIC
    ${LMDB_SRCS}
)

target_include_directories(
  eden_lmdb
  PUBLIC
    ${LMDB_INCLUDE_DIR}
)
target_link_libraries(
  eden_lmdb
  PUBLIC
    eden_utils
    Folly::folly
    ${LMDB_LIBRARIES}
)
