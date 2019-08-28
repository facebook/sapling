# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Find PrjFS
#
# This package sets:
# Prjfs_FOUND - Whether PrjFS was found
# PRJFS_INCLUDE_DIR - The include directory for Prjfs
# PRJFS_LIBRARY - The Prjfs library

include(FindPackageHandleStandardArgs)

find_path(PRJFS_INCLUDE_DIR NAMES ProjectedFSLib.h PATHS "facebook/third-party/prjfs" "D:/edenwin64/prjfs")
find_library(PRJFS_LIBRARY NAMES ProjectedFSLib.lib PATHS "facebook/third-party/prjfs" "D:/edenwin64/prjfs")
find_package_handle_standard_args(
  Prjfs
  PRJFS_INCLUDE_DIR
  PRJFS_LIBRARY
)

if(Prjfs_FOUND)
  add_library(ProjectedFS INTERFACE)
  target_include_directories(ProjectedFS INTERFACE "${PRJFS_INCLUDE_DIR}")
  target_link_libraries(ProjectedFS INTERFACE "${PRJFS_LIBRARY}")
endif()
