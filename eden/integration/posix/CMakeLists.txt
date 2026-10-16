# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB POSIX_INTEGRATION_SRCS "*.py")

find_package(FBThrift CONFIG REQUIRED)

add_fb_python_unittest(
  posix_integration_tests.egg
  TYPE "pex"
  SOURCES ${POSIX_INTEGRATION_SRCS}
  WORKING_DIRECTORY "${CMAKE_BINARY_DIR}"
  DEPENDS
    FBThrift::thrift_python_cpp
    Folly::folly_python_cpp
    eden_integration_lib
  NORMAL_DEPENDS
    FBThrift::thriftcpp2
    Folly::folly
    fmt::fmt
    glog::glog
  ENV
    "CMAKE_SOURCE_DIR=${CMAKE_SOURCE_DIR}"
)
