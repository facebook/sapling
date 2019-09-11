# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if (WIN32)
  # We should try detaching ServerState from the inodes
  # so that we don't need to make the Windows code depend
  # on it.
  add_library(
    eden_inodes STATIC
      ServerState.cpp
  )
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_journal
      Folly::folly
  )
else()
  file(GLOB INODES_SRCS "*.cpp")
  add_library(
    eden_inodes STATIC
      ${INODES_SRCS}
  )
  target_link_libraries(
    eden_inodes
    PUBLIC
      eden_config
      eden_fuse
      eden_journal
      eden_model_git
      eden_overlay_thrift_cpp
      eden_overlay
      eden_service
      eden_store
      eden_takeover_thrift
      eden_tracing
      eden_utils
  )
endif()

add_subdirectory(overlay)
