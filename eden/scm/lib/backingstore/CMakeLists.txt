# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# The crate builds into rust_edenfs_staticlib, in eden/fs's Cargo workspace,
# which can resolve another version of cxx than eden/scm's.
rust_cxx_bridge(
  backingstore
  "src/ffi.rs"
  LIBS
  fmt::fmt
  Folly::folly
  CXX_VERSION_DIR
  "${PROJECT_SOURCE_DIR}/eden/fs/rust/edenfs-staticlib"
)

# Open-source builds declare folly::IOBuf in a second bridge, see src/oss.rs.
rust_cxx_bridge(
  backingstore_iobuf
  "src/oss.rs"
  LIBS
  Folly::folly
  CXX_VERSION_DIR
  "${PROJECT_SOURCE_DIR}/eden/fs/rust/edenfs-staticlib"
)

file(GLOB C_API_SRCS "src/*.cpp")
file(GLOB C_API_HDRS "include/*.h")
target_sources(
  backingstore
  PRIVATE
  "${C_API_SRCS}"
)
set_target_properties(
  backingstore
  PROPERTIES
  PUBLIC_HEADER
  "${C_API_HDRS}"
)

target_include_directories(
  backingstore
  PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
include(generated_targets.cmake)
target_link_libraries(
  backingstore
  PUBLIC
  edencommon::edencommon_os
  ${SAPLING_NATIVE_BACKINGSTORE_DEPS}
  rust_edenfs_staticlib
  backingstore_iobuf
  fmt::fmt
  Folly::folly
)

rust_cxx_header(
  rust_cxx
  LIBS
  rust_edenfs_staticlib
  CXX_VERSION_DIR
  "${PROJECT_SOURCE_DIR}/eden/fs/rust/edenfs-staticlib"
)

# curl used in the Rust crate has its own copy of curl compiled and it uses
# Crypt32 and Secur32 on Windows. We need to declare the link dependencies here
# to avoid linker errors.
if (WIN32)
  target_link_libraries(
    backingstore
    PRIVATE
    ntdll
    Crypt32
    Secur32
    Ncrypt
    winhttp
  )
else()
  target_link_libraries(
    backingstore
    PRIVATE
      CURL::libcurl
  )
endif()

# Reqwest links against the security framework.
if (APPLE)
  target_link_libraries(
    backingstore
    PRIVATE
    "-framework Security"
  )
endif()

install(
  TARGETS backingstore
  EXPORT mercurial
  LIBRARY DESTINATION lib
  PUBLIC_HEADER DESTINATION eden/scm/lib/backingstore/include
)
