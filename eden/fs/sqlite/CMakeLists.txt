# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB SQLITE_SRCS "*.cpp")
add_library(
  eden_sqlite STATIC
    ${SQLITE_SRCS}
)
target_include_directories(
  eden_sqlite
  PUBLIC
    ${SQLITE3_INCLUDE_DIR}
)
target_link_libraries(
  eden_sqlite
  PUBLIC
    eden_utils
    Folly::folly
    ${SQLITE3_LIBRARY}
)
