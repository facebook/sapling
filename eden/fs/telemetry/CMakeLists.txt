# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TELEMETRY_SRCS "*.cpp")

if (WIN32)
  list(
    REMOVE_ITEM TELEMETRY_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/ScubaStructuredLogger.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/SubprocessScribeLogger.cpp
  )
endif()

add_library(
  eden_telemetry STATIC
    ${TELEMETRY_SRCS}
)
target_link_libraries(
  eden_telemetry
  PUBLIC
    eden_config
    eden_utils
    fb303::fb303
    Folly::folly
)
