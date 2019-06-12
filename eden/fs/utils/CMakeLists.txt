# Copyright (c) 2019-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

file(GLOB UTILS_SRCS "*.cpp")

if (WIN32)
  list(
    REMOVE_ITEM UTILS_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/CoverageSet.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/FutureUnixSocket.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/IoFuture.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcessAccessLog.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcessNameCache.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/ProcUtil.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/SSLContext.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/UnixSocket.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/XAttr.cpp
  )
endif()

add_library(
  eden_utils STATIC
    ${UTILS_SRCS}
)

target_link_libraries(
  eden_utils
  PUBLIC
    eden_service
    Folly::folly
)

if (NOT WIN32)
  add_subdirectory(test)
endif()
