# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_nfs_rpc STATIC
    "Rpc.cpp" "StreamClient.cpp" "Rpc.h" "StreamClient.h"
)

target_link_libraries(
  eden_nfs_rpc
  PUBLIC
    eden_nfs_xdr
    Folly::folly
)

add_library(
  eden_nfs_rpc_server STATIC
    "Server.cpp" "Server.h"
)

target_link_libraries(
  eden_nfs_rpc_server
  PUBLIC
    eden_nfs_rpc
    eden_nfs_portmap
    Folly::folly
  PRIVATE
    eden_utils
)

add_subdirectory(test)
