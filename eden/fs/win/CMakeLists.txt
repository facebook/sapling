# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB CONFIG_SRCS "mount/*.cpp" "mount/*.h")
add_library(
  eden_win STATIC
    ${CONFIG_SRCS}
)
target_link_libraries(
  eden_win
  PUBLIC
    eden_inodes
    eden_journal
    eden_model
    eden_store
    eden_telemetry
    eden_utils
    eden_service_thrift_util
    eden_win_utils
    Folly::folly
    ProjectedFS
    cpptoml
)

add_subdirectory(utils)
