# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

rust_static_library(rust_bindings CRATE edenapithin)
install_rust_static_library(
  rust_bindings
  EXPORT mercurial
  INSTALL_DIR lib
)

file(GLOB C_API_SRCS "c_api/*.cpp")
add_library(edenapithin "${C_API_SRCS}")
set_target_properties(
  bindings
  PROPERTIES
  PUBLIC_HEADER
  "c_api/RustEdenApi.h;c_api/EdenApiThinWrapper.h"
)
target_include_directories(edenapithin PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  edenapithin
  PRIVATE
  rust_bindings
  Folly::folly
)

# curl used in the Rust crate has its own copy of curl compiled and it uses
# Crypt32 and Secur32 on Windows. We need to declare the link dependencies here
# to avoid linker errors.
if (WIN32)
  target_link_libraries(
    edenapithin
    PRIVATE
    Crypt32
    Secur32
    Ncrypt
  )
endif()

# Reqwest links against the security framework.
if (APPLE)
  target_link_libraries(
    edenapithin
    PRIVATE
    "-framework Security"
  )
endif()

install(
  TARGETS edenapithin
  EXPORT mercurial
  LIBRARY DESTINATION ${LIB_INSTALL_DIR}
  PUBLIC_HEADER DESTINATION "include/eden/hg-server/lib/edenapi/bindings/c_api"
)
