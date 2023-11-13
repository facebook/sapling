# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB LMDB_CATALOG_SRCS "*.cpp")

add_library(
  eden_lmdb_catalog STATIC
    ${LMDB_CATALOG_SRCS}
)

target_link_libraries(
  eden_lmdb_catalog
  PUBLIC
    eden_lmdb

  PRIVATE
    eden_config
    eden_inodes_inodenumber
    eden_overlay_thrift_cpp
    eden_utils
    Folly::folly
    edencommon::edencommon_utils
)
