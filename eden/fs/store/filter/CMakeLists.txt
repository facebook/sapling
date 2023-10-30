# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO(cuev):
# This is the start of what's needed to support HgSparseFilters in the OSS
# build. However, I ran into the issue of needing to make CMake understand
# where to locate rust/cxx.h. This would require significant time, so it's
# now a problem for another time. It's not critical that we build this in
# our open source build just yet.
file(GLOB STORE_FILTER_SRCS "*.cpp")
list(
  REMOVE_ITEM STORE_FILTER_SRCS
  "${CMAKE_CURRENT_SOURCE_DIR}/HgSparseFilter.cpp"
)

add_library(
  eden_store_filter STATIC
    ${STORE_FILTER_SRCS}
)

target_link_libraries(eden_store_filter PUBLIC
    eden_config
    eden_model
    eden_utils
    edenfs_ffi
    ${RE2_LIBRARY})
