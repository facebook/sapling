# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Allow symlinking the fb-mercurial dir in directly
if (IS_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/fb-mercurial)
  set(FB_MERCURIAL_DIR "${CMAKE_CURRENT_SOURCE_DIR}/fb-mercurial")
else()
  # Otherwise, look for where getdeps has "installed" the source
  find_file(EDENSCM_DATAPACKSTORE_CPP "edenscm/hgext/extlib/cstore/datapackstore.cpp")
  if (EDENSCM_DATAPACKSTORE_CPP)
    get_filename_component(CSTORE_DIR "${EDENSCM_DATAPACKSTORE_CPP}" DIRECTORY)
    get_filename_component(FB_MERCURIAL_DIR "${CSTORE_DIR}/../../../../" REALPATH)
  endif()
endif()

# TODO: once we've gotten the rust datapack code integrated and
# building, update getdeps.py to optionally pull from the fb-mercurial
# repo on github and adjust this logic to use either the code from
# the local fbsource repo when building at FB, or from the
# external dir when building the OSS build.
if (IS_DIRECTORY "${FB_MERCURIAL_DIR}")
  find_package(LZ4 MODULE REQUIRED)
  include_directories(${FB_MERCURIAL_DIR})
  add_library(
    libmpatch
    STATIC
      ${FB_MERCURIAL_DIR}/edenscm/mercurial/mpatch.c
  )

  add_library(
    buffer
    STATIC
      ${FB_MERCURIAL_DIR}/lib/clib/buffer.c
  )

  add_library(
    datapack
    STATIC
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/cstore/datapackstore.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/cstore/deltachain.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/cstore/uniondatapackstore.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_entry.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/ctreemanifest/manifest_ptr.cpp
      ${FB_MERCURIAL_DIR}/edenscm/hgext/extlib/ctreemanifest/treemanifest.cpp
      ${FB_MERCURIAL_DIR}/lib/cdatapack/cdatapack.c
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
    ${OPENSSL_INCLUDE_DIR}
    ${LZ4_INCLUDE_DIR}
  )
  if (WIN32)
  # We need to define EDEN_WIN to include the correct definition of mman.h, 
  # which is different for Mercurial Windows and Eden Windows.
    target_compile_definitions(datapack PUBLIC -DEDEN_WIN)
  endif()
else()
  message(FATAL_ERROR "fb-mercurial treemanifest support not found")
endif()
