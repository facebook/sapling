# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

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
