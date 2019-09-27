# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FindPackageHandleStandardArgs)

# Allow symlinking the fb-mercurial dir in directly
if (IS_DIRECTORY ${CMAKE_SOURCE_DIR}/fb-mercurial)
  set(EDENSCM_DIR "${CMAKE_SOURCE_DIR}/fb-mercurial")
else()
  # Otherwise, look for where getdeps has "installed" the source
  find_file(
    EDENSCM_DATAPACKSTORE_CPP
    "edenscm/hgext/extlib/cstore/datapackstore.cpp"
  )
  if (EDENSCM_DATAPACKSTORE_CPP)
    get_filename_component(CSTORE_DIR "${EDENSCM_DATAPACKSTORE_CPP}" DIRECTORY)
    get_filename_component(EDENSCM_DIR "${CSTORE_DIR}/../../../../" REALPATH)
  endif()
  if (NOT IS_DIRECTORY "${EDENSCM_DIR}")
    set(EDENSCM_DIR "EDENSCM_DIR-NOTFOUND")
  endif()
endif()

find_package_handle_standard_args(
  EdenSCM
  REQUIRED_VARS EDENSCM_DIR
  FAIL_MESSAGE "Unable to find EdenSCM fb-mercurial directory"
)
