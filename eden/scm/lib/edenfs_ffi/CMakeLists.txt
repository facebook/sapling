# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Due to some tools changing '-' to '_' when building,
# this library/crate is named using an '_' so that it can link successfully.
rust_static_library(rust_edenfs_ffi CRATE edenfs_ffi USE_CXX_INCLUDE)
install_rust_static_library(
  rust_edenfs_ffi
  INSTALL_DIR lib
)

file(GLOB edenfs_ffi_srcs "src/ffi.cpp")
add_library(edenfs_ffi "${edenfs_ffi_srcs}")
set_target_properties(
  edenfs_ffi
  PROPERTIES
  PUBLIC_HEADER
  "src/ffi.h"
)
target_include_directories(
  edenfs_ffi
  PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  edenfs_ffi
  PRIVATE
  rust_edenfs_ffi
  Folly::folly
  eden_model
  eden_utils
  ${RE2_LIBRARY}
)

# Windows-only configuration
if(WIN32)
  target_link_libraries(
    edenfs_ffi
    userenv
    ws2_32
    bcrypt
  )
  set_target_properties(
    edenfs_ffi
    PROPERTIES
    MSVC_RUNTIME_LIBRARY "MultiThreadedDLL"
    RUNTIME_OUTPUT_DIRECTORY_DEBUG ${CMAKE_BINARY_DIR}
    RUNTIME_OUTPUT_DIRECTORY_RELEASE ${CMAKE_BINARY_DIR}
  )
endif()

install(
  TARGETS edenfs_ffi
  LIBRARY DESTINATION ${LIB_INSTALL_DIR}
  PUBLIC_HEADER DESTINATION "include/eden/scm/lib/edenfs_ffi/src"
)
