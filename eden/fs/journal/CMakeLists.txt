# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB JOURNAL_SRCS "*.cpp")
add_library(
  eden_journal STATIC
    ${JOURNAL_SRCS}
)
target_link_libraries(
  eden_journal
  PUBLIC
    eden_model
    eden_telemetry
    eden_utils
    streamingeden_thrift_cpp
)

add_subdirectory(test)
