# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


add_library(
  portmap_rpc STATIC
    "RpcbindRpc.cpp" "RpcbindRpc.h"
)

target_link_libraries(
  portmap_rpc
  PUBLIC
    eden_nfs_rpc
    Folly::folly
    edencommon::edencommon_utils
)

add_library(
  eden_nfs_portmap STATIC
    "PortmapClient.cpp" "PortmapClient.h"
)

target_link_libraries(
  eden_nfs_portmap
  PUBLIC
    portmap_rpc
    Folly::folly
)

add_library(
  eden_nfs_portmap_server STATIC
    "Rpcbindd.cpp" "Rpcbindd.h"
)

target_link_libraries(
  eden_nfs_portmap_server
  PUBLIC
    portmap_rpc
    Folly::folly
)
