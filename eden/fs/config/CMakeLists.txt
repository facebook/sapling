# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_config_thrift
  eden_config.thrift
  LANGUAGES cpp py
  PY_NAMESPACE facebook.eden.eden_config
)

file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_config STATIC
    ${CONFIG_SRCS}
)
target_link_libraries(
  eden_config
  PUBLIC
    eden_config_thrift_cpp
    eden_model
    eden_utils
    cpptoml
)
