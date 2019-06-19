# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_mount STATIC
    ${CONFIG_SRCS}
)


target_link_libraries(
  eden_win_mount
  PUBLIC
    eden_service_thrift
    Folly::folly
    ProjectedFS
)
