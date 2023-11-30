# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_FILTER_SRCS "*.cpp")

add_library(
  eden_store_filter STATIC
    ${STORE_FILTER_SRCS}
)

target_link_libraries(eden_store_filter PUBLIC
    eden_config
    eden_model
    eden_utils
    edenfs_ffi
    ${RE2_LIBRARY})
