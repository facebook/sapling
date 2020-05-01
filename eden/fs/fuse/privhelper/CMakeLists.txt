# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

if(NOT WIN32)
  file(GLOB PRIVHELPER_SRCS "*.cpp")
else()
  list(
    APPEND PRIVHELPER_SRCS
    "PrivHelper.cpp"
    "PrivHelperImpl.cpp"
  )
endif()

add_library(
  eden_fuse_privhelper STATIC
    ${PRIVHELPER_SRCS}
)

target_link_libraries(
  eden_fuse_privhelper
  PUBLIC
    eden_utils
    Folly::folly
)

if (APPLE)
  target_link_libraries(
    eden_fuse_privhelper
    PUBLIC
      "-framework CoreFoundation"
      "-framework IOKit"
  )
endif()

if (SELINUX_INCLUDE_DIR)
  target_include_directories(
    eden_fuse_privhelper
    PRIVATE
      ${SELINUX_INCLUDE_DIR}
  )
  target_link_libraries(
    eden_fuse_privhelper
    PUBLIC
      ${SELINUX_LIBRARIES}
  )
endif()

if(NOT WIN32)
  add_subdirectory(test)
endif()
