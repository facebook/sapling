# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB TRACING_SRCS "*.cpp")
add_library(
  eden_tracing STATIC
    ${TRACING_SRCS}
)
target_link_libraries(
  eden_tracing
  PUBLIC
    eden_utils
    Folly::folly
)
