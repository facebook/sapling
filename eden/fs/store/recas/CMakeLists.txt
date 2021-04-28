# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_RE_CAS_SRCS "*.cpp")
add_library(
  eden_store_recas STATIC
    ${STORE_RE_CAS_SRCS}
)
target_link_libraries(
  eden_store_recas
  PUBLIC
    backingstore
    eden_store
)

if (NOT WIN32)
  add_subdirectory(test)
endif()
