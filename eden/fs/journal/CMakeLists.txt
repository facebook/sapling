# Copyright (c) Facebook, Inc. and its affiliates.
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
    eden_tracing
    eden_utils
    streamingeden_thrift_cpp
)
