# TODO: once we've gotten the rust datapack code integrated and
# building, update getdeps.py to optionally pull from the fb-mercurial
# repo on github and adjust this logic to use either the code from
# the local fbsource repo when building at FB, or from the
# external dir when building the OSS build.
if (IS_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}/fb-mercurial)
  include_directories(${CMAKE_CURRENT_SOURCE_DIR}/fb-mercurial)
  add_library(
    libmpatch
    STATIC
      fb-mercurial/mercurial/mpatch.c
  )

  add_library(
    buffer
    STATIC
      fb-mercurial/lib/clib/buffer.c
  )

  add_library(
    datapack
    STATIC
      fb-mercurial/hgext/extlib/cstore/datapackstore.cpp
      fb-mercurial/hgext/extlib/cstore/deltachain.cpp
      fb-mercurial/hgext/extlib/cstore/uniondatapackstore.cpp
      fb-mercurial/hgext/extlib/ctreemanifest/manifest.cpp
      fb-mercurial/hgext/extlib/ctreemanifest/manifest_entry.cpp
      fb-mercurial/hgext/extlib/ctreemanifest/manifest_fetcher.cpp
      fb-mercurial/hgext/extlib/ctreemanifest/manifest_ptr.cpp
      fb-mercurial/hgext/extlib/ctreemanifest/treemanifest.cpp
      fb-mercurial/lib/cdatapack/cdatapack.c
  )
  target_link_libraries(
    datapack
    PUBLIC
      libmpatch
      buffer
      ${OPENSSL_LIBRARIES}
  )
  target_include_directories(
    datapack
    PUBLIC
    ${OPENSSL_INCLUDE_DIR}
  )

  set(EDEN_HAVE_HG_TREEMANIFEST ON)
endif()
