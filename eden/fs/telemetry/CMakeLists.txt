# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TELEMETRY_SRCS "*.cpp")


add_library(
  eden_telemetry STATIC
    ${TELEMETRY_SRCS}
)
target_link_libraries(
  eden_telemetry
  PUBLIC
    eden_config
    eden_service_thrift_util
    eden_utils
    edencommon::edencommon_telemetry
    fb303::fb303
    Folly::folly
)
