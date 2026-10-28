# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(generated_targets.cmake)
eden_add_directory_library(eden_nfs_nfsd_rpc)
eden_add_directory_library(eden_nfs_utils)

add_subdirectory(portmap)
add_subdirectory(rpc)
add_subdirectory(xdr)
add_subdirectory(test)
add_subdirectory(testharness)
