# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB DIGEST_SRCS "*.cpp")
add_library(
  eden_digest STATIC
    ${DIGEST_SRCS}
)

target_link_libraries(
  eden_digest
  PUBLIC
    BLAKE3::blake3
    Folly::folly
)

add_subdirectory(test)
