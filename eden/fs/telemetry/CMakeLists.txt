# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TRACING_SRCS "*.cpp")
add_library(
  eden_telemetry STATIC
    ${TRACING_SRCS}
)
target_link_libraries(
  eden_telemetry
  PUBLIC
    eden_config
    eden_utils
    fb303::fb303
    Folly::folly
)
