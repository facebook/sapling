# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_cpp_library(
  eden_takeover_thrift
  takeover.thrift
)

if (NOT WIN32)
  file(GLOB TAKEOVER_SRCS "*.cpp")
  add_library(
    eden_takeover STATIC
      ${TAKEOVER_SRCS}
  )
  target_link_libraries(
    eden_takeover
    PUBLIC
      eden_fuse
      eden_utils
      eden_takeover_thrift
  )
endif()
