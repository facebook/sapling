# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB FUSE_SRCS "*.cpp")
add_library(
  eden_fuse STATIC
    ${FUSE_SRCS}
)
target_link_libraries(
  eden_fuse
  PUBLIC
    eden_fuse_privhelper
    eden_inodes_inodenumber
    eden_notifications
    eden_request_context
    eden_telemetry
    Folly::folly
    edencommon::edencommon_utils
)

add_subdirectory(privhelper)
