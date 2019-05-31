# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_store STATIC
    ${CONFIG_SRCS}
)

target_link_libraries(
  eden_win_store
  PUBLIC
   Folly::folly
    ProjectedFS
)
