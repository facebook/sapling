# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Due to some tools changing '-' to '_' when building,
# this library/crate is named using an '_' so that it can link successfully.
rust_static_library(rust_redirect_ffi CRATE redirect_ffi USE_CXX_INCLUDE)

install_rust_static_library(
  rust_redirect_ffi
  INSTALL_DIR lib
)

rust_cxx_bridge(
  redirect_ffi
  "src/lib.rs"
  LIBS
  fmt::fmt
  Folly::folly
)

file(GLOB C_API_SRCS "src/*.cpp")
file(GLOB C_API_HDRS "include/*.h")
target_sources(
  redirect_ffi
  PRIVATE
  "${C_API_SRCS}"
)

set_target_properties(
  redirect_ffi
  PROPERTIES
  PUBLIC_HEADER
  "${C_API_HDRS}"
)

target_include_directories(
  redirect_ffi
  PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  redirect_ffi
  PUBLIC
  rust_redirect_ffi
  Folly::folly
  edencommon::edencommon_utils
  fmt::fmt
)

install(
  TARGETS redirect_ffi
  LIBRARY DESTINATION lib
  PUBLIC_HEADER DESTINATION eden/scm/lib/redirect_ffi/include
)
