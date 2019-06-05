# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

add_thrift_cpp2_library(
  eden_config_thrift
  eden_config.thrift
)

file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_config STATIC
    ${CONFIG_SRCS}
)
target_link_libraries(
  eden_config
  PUBLIC
    eden_config_thrift
    eden_utils
)
target_include_directories(
  eden_config
  PRIVATE
    ${CPPTOML_INCLUDE_DIR}
)
