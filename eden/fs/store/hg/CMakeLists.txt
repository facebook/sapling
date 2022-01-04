# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_HG_SRCS "*.cpp")

add_library(
  eden_store_hg STATIC
    ${STORE_HG_SRCS}
)

target_link_libraries(
  eden_store_hg
  PUBLIC
    backingstore
    eden_config
    eden_model
    eden_store
    eden_utils
    ${RE2_LIBRARY}
)

if (NOT WIN32)
  add_subdirectory(test)
endif()
