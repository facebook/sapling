# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if (NOT WIN32)
  file(GLOB OVERLAY_SRCS "*.cpp")
  list(
    REMOVE_ITEM OVERLAY_SRCS
    "${CMAKE_CURRENT_SOURCE_DIR}/eden_fsck.cpp"
  )
  add_library(
    eden_fscatalog STATIC
      ${OVERLAY_SRCS}
  )
  target_link_libraries(
    eden_fscatalog
    PUBLIC
      eden_overlay_thrift_cpp
      eden_fuse
      eden_utils
  )
endif()
