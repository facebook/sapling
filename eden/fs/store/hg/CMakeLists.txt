# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

file(GLOB STORE_HG_SRCS "*.cpp")

if (NOT EDEN_HAVE_RUST_DATAPACK)
  list(
    REMOVE_ITEM STORE_HG_SRCS
      ${CMAKE_CURRENT_SOURCE_DIR}/HgDatapackStore.cpp
  )
endif()

add_library(
  eden_store_hg STATIC
    ${STORE_HG_SRCS}
)

list(
  APPEND EDEN_STORE_HG_OPTIONAL_DEPS
    datapack
    eden_config
)
if (NOT WIN32)
  list(APPEND EDEN_STORE_HG_OPTIONAL_DEPS eden_store_mononoke)
endif()

if(EDEN_HAVE_RUST_DATAPACK)
  list(
    APPEND EDEN_STORE_HG_OPTIONAL_DEPS
      Mercurial::backingstore
  )
endif()

target_link_libraries(
  eden_store_hg
  PUBLIC
    eden_model
    eden_store
    eden_utils
    ${EDEN_STORE_HG_OPTIONAL_DEPS}
)

if (NOT WIN32)
  add_subdirectory(test)
endif()
