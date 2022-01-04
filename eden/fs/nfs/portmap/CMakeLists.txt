# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_nfs_portmap STATIC
    "PortmapClient.cpp" "PortmapClient.h"
)

target_link_libraries(
  eden_nfs_portmap
  PUBLIC
    eden_nfs_rpc
    Folly::folly
)
