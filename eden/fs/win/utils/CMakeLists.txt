# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_utils STATIC
    ${CONFIG_SRCS}
)

target_link_libraries(
  eden_win_utils
  PUBLIC
    eden_service_thrift
    Userenv.lib
    Folly::folly
)
