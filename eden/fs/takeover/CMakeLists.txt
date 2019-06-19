# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_thrift_cpp2_library(
  eden_takeover_thrift
  takeover.thrift
)

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
