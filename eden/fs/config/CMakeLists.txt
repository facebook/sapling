# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_config_thrift
  eden_config.thrift
  LANGUAGES cpp py
  PY_NAMESPACE facebook.eden.eden_config
)

if (EDEN_HAVE_THRIFT_PYTHON)
  add_fbthrift_python_library(
    eden_config_thrift_python
    eden_config.thrift
    NAMESPACE eden.fs.config
  )
endif()

include(generated_targets.cmake)
eden_add_directory_library(eden_config)

if(BUILD_TESTS)
  add_subdirectory(test)
endif()
