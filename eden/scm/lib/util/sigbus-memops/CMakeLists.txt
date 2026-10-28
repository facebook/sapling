# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

add_library(
  sigbus_memops STATIC
    native/sigbus_memops.c
    native/sigbus_memops_inline.c
)
target_include_directories(
  sigbus_memops
  PUBLIC
    ${CMAKE_CURRENT_SOURCE_DIR}
  PRIVATE
    ${CMAKE_CURRENT_SOURCE_DIR}/native
)
