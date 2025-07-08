# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB INTEGRATION_LIB_SRCS "*.py")
if(pexpect_FOUND)
  list(APPEND INTEGRATION_LIB_DEPENDENCIES pexpect::pexpect)
else()
  list(REMOVE_ITEM INTEGRATION_LIB_SRCS pexpect.py)
endif()

if(IS_FB_BUILD)
  list(APPEND INTEGRATION_LIB_DEPENDENCIES eden_integration_fb_lib)
endif()

add_fb_python_library(
  eden_integration_lib
  SOURCES ${INTEGRATION_LIB_SRCS}
  DEPENDS
    edenfsctl.real.main_lib
    eden_config_thrift_py3
    eden_service_thrift_py3
    eden_py
    eden_test_support
    fb303::fb303_core_py3
    ${INTEGRATION_LIB_DEPENDENCIES}
)
