# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if(NOT WIN32)
  file(GLOB FUSE_SRCS "*.cpp")
  add_library(
    eden_fuse STATIC
      ${FUSE_SRCS}
  )
  target_link_libraries(
    eden_fuse
    PUBLIC
      eden_fuse_privhelper
      eden_notifications
      eden_telemetry
      Folly::folly
  )
endif()

add_subdirectory(privhelper)
