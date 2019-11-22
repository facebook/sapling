# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

add_library(
  eden_buffer
  STATIC
    buffer.c
)
target_include_directories(
  eden_buffer
  PUBLIC
    "${CMAKE_SOURCE_DIR}/eden/scm"
)
target_link_libraries(
  eden_buffer
  PUBLIC
    OpenSSL::Crypto
)
if (WIN32)
  # We need to define EDEN_WIN to include the correct definition of mman.h,
  # which is different for Mercurial Windows and Eden Windows.
  target_compile_definitions(eden_buffer PUBLIC EDEN_WIN)
endif()
