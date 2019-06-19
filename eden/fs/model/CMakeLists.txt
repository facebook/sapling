# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB MODEL_SRCS "*.cpp")
add_library(
  eden_model STATIC
    ${MODEL_SRCS}
)
target_link_libraries(
  eden_model
  PUBLIC
    eden_utils
    Folly::folly
)

add_subdirectory(git)
