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

rust_cxx_bridge(
  edenfs_ffi
  "src/lib.rs"
  LIBS
  fmt::fmt
  Folly::folly
)

file(GLOB C_API_SRCS "src/*.cpp")
file(GLOB C_API_HDRS "include/*.h")
target_sources(
  edenfs_ffi
  PRIVATE
  "${C_API_SRCS}"
)

set_target_properties(
  edenfs_ffi
  PROPERTIES
  PUBLIC_HEADER
  "${C_API_HDRS}"
)

target_include_directories(
  edenfs_ffi
  PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  edenfs_ffi
  PUBLIC
  rust_edenfs_ffi
  Folly::folly
  edencommon::edencommon_utils
  fmt::fmt
)

# curl used in the Rust crate has its own copy of curl compiled and it uses
# Crypt32 and Secur32 on Windows. We need to declare the link dependencies here
# to avoid linker errors.
if (WIN32)
  target_link_libraries(
    edenfs_ffi
    PRIVATE
    ntdll
    Crypt32
    Secur32
    Ncrypt
    winhttp
  )
else()
  target_link_libraries(
    edenfs_ffi
    PRIVATE
      CURL::libcurl
  )
endif()

install(
  TARGETS edenfs_ffi
  LIBRARY DESTINATION lib
  PUBLIC_HEADER DESTINATION eden/scm/lib/edenfs_ffi/include
)
