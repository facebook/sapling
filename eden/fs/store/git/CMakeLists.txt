# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_GIT_SRCS "*.cpp")
add_library(
  eden_store_git STATIC
    ${STORE_GIT_SRCS}
)
target_link_libraries(
  eden_store_git
  PUBLIC
    eden_store
    libgit2
)
