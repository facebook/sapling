# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_fbthrift_library(
  eden_service_thrift
  eden.thrift
  LANGUAGES cpp py
  PY_NAMESPACE facebook.eden
  SERVICES
    EdenService
  DEPENDS
    eden_config_thrift
    fb303::fb303_thrift
)

if (EDEN_HAVE_THRIFT_PYTHON)
  add_fbthrift_python_library(
    eden_service_thrift_python
    eden.thrift
    NAMESPACE eden.fs.service
    SERVICES
      EdenService
    DEPENDS
      eden_config_thrift_python
      fb303::fb303_core_thrift_python
  )
endif()

add_fbthrift_library(
  streamingeden_thrift
  streamingeden.thrift
  LANGUAGES cpp
  SERVICES
    StreamingEdenService
  DEPENDS
    eden_service_thrift
)

include(generated_targets.cmake)
eden_add_directory_library(eden_service)
