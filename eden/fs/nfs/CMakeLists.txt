# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_nfs_mountd STATIC
    "Mountd.cpp" "Mountd.h"
)

target_link_libraries(
  eden_nfs_mountd
  PUBLIC
    eden_nfs_rpc_server
  PRIVATE
    eden_nfs_mountd_rpc
    Folly::folly
)

add_library(
  eden_nfs_mountd_rpc STATIC
    "MountdRpc.cpp" "MountdRpc.h"
)

target_link_libraries(
  eden_nfs_mountd_rpc
  PUBLIC
    eden_inodes_inodenumber
    eden_nfs_rpc
)

add_subdirectory(portmap)
add_subdirectory(rpc)
add_subdirectory(xdr)
