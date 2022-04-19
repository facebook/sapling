# Copyright (c) Meta Platforms, Inc. and affiliates.
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
    eden_inodes_inodenumber
    eden_nfs_rpc_server
    eden_utils
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
    eden_nfs_nfsd_rpc
    eden_nfs_rpc
)

add_library(
  eden_nfs_nfsd_rpc STATIC
    "NfsdRpc.cpp" "NfsdRpc.h"
)

target_link_libraries(
  eden_nfs_nfsd_rpc
  PUBLIC
    eden_inodes_inodenumber
    eden_nfs_rpc
)

add_library(
  eden_nfs_utils STATIC
    "NfsUtils.cpp" "NfsUtils.h"
)


target_link_libraries(
  eden_nfs_utils
  PRIVATE
    eden_nfs_nfsd_rpc
    Folly::folly
)


add_library(
  eden_nfs_nfsd3 STATIC
    "Nfsd3.cpp" "Nfsd3.h" "NfsRequestContext.cpp" "NfsRequestContext.h"
)

target_link_libraries(
  eden_nfs_nfsd3
  PUBLIC
    eden_nfs_dispatcher
    eden_nfs_rpc_server
  PRIVATE
    eden_nfs_nfsd_rpc
    eden_nfs_utils
    Folly::folly
)

add_library(
  eden_nfs_server STATIC
    "NfsServer.cpp" "NfsServer.h"
)

target_link_libraries(
  eden_nfs_server
  PUBLIC
    eden_nfs_mountd
    eden_nfs_nfsd3
  PRIVATE
    Folly::folly
)

add_library(
  eden_nfs_dispatcher STATIC
    "NfsDispatcher.cpp" "NfsDispatcher.h"
)

target_link_libraries(
  eden_nfs_dispatcher
  PUBLIC
    eden_inodes_inodenumber
    eden_inode_metadata
    eden_nfs_dirlist
    eden_store
    eden_utils
)

add_library(
  eden_nfs_dirlist STATIC
    "DirList.cpp" "DirList.h"
)

target_link_libraries(
  eden_nfs_dirlist
  PUBLIC
    eden_nfs_nfsd_rpc
    eden_inodes_inodenumber
)

add_subdirectory(portmap)
add_subdirectory(rpc)
add_subdirectory(xdr)
add_subdirectory(test)
add_subdirectory(testharness)
