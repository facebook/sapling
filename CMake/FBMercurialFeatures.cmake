# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO: once we've gotten the rust datapack code integrated and
# building, update getdeps.py to optionally pull from the fb-mercurial
# repo on github and adjust this logic to use either the code from
# the local fbsource repo when building at FB, or from the
# external dir when building the OSS build.
find_package(LZ4 MODULE REQUIRED)
add_library(
  libmpatch
  STATIC
    ${EDENSCM_DIR}/edenscm/mercurial/mpatch.c
)
target_include_directories(
  libmpatch
  PUBLIC
    ${EDENSCM_DIR}
)

add_library(
  buffer
  STATIC
    ${EDENSCM_DIR}/lib/clib/buffer.c
)
target_include_directories(
  buffer
  PUBLIC
    ${EDENSCM_DIR}
)

add_library(
  datapack
  STATIC
    ${EDENSCM_DIR}/edenscm/hgext/extlib/cstore/datapackstore.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/cstore/deltachain.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/cstore/uniondatapackstore.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_entry.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_ptr.cpp
    ${EDENSCM_DIR}/edenscm/hgext/extlib/ctreemanifest/treemanifest.cpp
    ${EDENSCM_DIR}/lib/cdatapack/cdatapack.c
)
target_link_libraries(
  datapack
  PUBLIC
    libmpatch
    buffer
    ${OPENSSL_LIBRARIES}
    ${LZ4_LIBRARY}
)
target_include_directories(
  datapack
  PUBLIC
    ${EDENSCM_DIR}
    ${OPENSSL_INCLUDE_DIR}
    ${LZ4_INCLUDE_DIR}
)
if (WIN32)
  # We need to define EDEN_WIN to include the correct definition of mman.h, 
  # which is different for Mercurial Windows and Eden Windows.
  target_compile_definitions(datapack PUBLIC -DEDEN_WIN)
endif()
