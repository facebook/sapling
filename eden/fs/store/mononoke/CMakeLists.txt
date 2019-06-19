# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# FIXME: need to pull in proxygen for this
file(GLOB STORE_MONONOKE_SRCS "*.cpp")

if (EDEN_WIN_NOMONONOKE)
  list(
    REMOVE_ITEM
    STORE_MONONOKE_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/MononokeHttpBackingStore.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/MononokeThriftBackingStore.cpp
  )
endif()

if (NOT EDEN_HAVE_CURL)
  list(
    REMOVE_ITEM
    STORE_MONONOKE_SRCS
    ${CMAKE_CURRENT_SOURCE_DIR}/CurlHttpClient.cpp
    ${CMAKE_CURRENT_SOURCE_DIR}/MononokeCurlBackingStore.cpp
  )
endif()

add_library(
  eden_store_mononoke STATIC
    ${STORE_MONONOKE_SRCS}
)

target_link_libraries(
  eden_store_mononoke
  PUBLIC
    eden_model
    eden_store
    eden_utils
)

if (CURL_FOUND)
  target_link_libraries(
    eden_store_mononoke
    PUBLIC
      curl
  )
endif()
