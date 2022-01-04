# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# The add_fbthrift_library() calls require CMake 3.8+
cmake_minimum_required(VERSION 3.8 FATAL_ERROR)
# We use the GoogleTest module if it is available (only in CMake 3.9+)
# It requires CMP0054 and CMP0057 to be enabled.
if (POLICY CMP0054)
  cmake_policy(SET CMP0054 NEW)
endif()
if (POLICY CMP0057)
  cmake_policy(SET CMP0057 NEW)
endif()

# Project information
project("eden" LANGUAGES CXX C)

set(EDEN_BUILD_REVISION "")
if(DEFINED ENV{FBSOURCE_DATE})
  # If set, we expect FBSOURCE_DATE to have the form "20200324.113140"
  set(PACKAGE_VERSION "$ENV{FBSOURCE_DATE}.0")
  set(EDEN_BUILD_REVISION "$ENV{FBSOURCE_HASH}")
else()
  find_program(GIT git)
  if(GIT)
    execute_process(
      COMMAND
      "${GIT}" "log" "-1" "--format=%H;%cd" "--date=format:%Y%m%d.%H%M%S.0"
      WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
      RESULT_VARIABLE git_result
      OUTPUT_VARIABLE git_data
      ERROR_VARIABLE git_err
      OUTPUT_STRIP_TRAILING_WHITESPACE
    )
    if(git_result EQUAL 0)
      list(GET git_data 0 EDEN_BUILD_REVISION)
      list(GET git_data 1 PACKAGE_VERSION)
    endif()
  endif()
endif()

set(
  EDEN_VERSION_OVERRIDE "" CACHE STRING
  "Use this version number instead of the default that would be selected"
)
if(EDEN_VERSION_OVERRIDE)
  set(PACKAGE_VERSION "${EDEN_VERSION_OVERRIDE}")
elseif(DEFINED ENV{EDEN_VERSION_OVERRIDE})
  set(PACKAGE_VERSION "$ENV{EDEN_VERSION_OVERRIDE}")
endif()
if(PACKAGE_VERSION)
  string(REPLACE "." ";" VERSION_FIELDS "${PACKAGE_VERSION}")
  list(LENGTH VERSION_FIELDS NUM_VERSION_FIELDS)
  if(NOT "${NUM_VERSION_FIELDS}" EQUAL 3)
    message(
      FATAL_ERROR
      "Bad version string \"${PACKAGE_VERSION}\": must be in X.Y.Z format"
    )
  endif()
  list(GET VERSION_FIELDS 0 EDEN_VERSION)
  list(GET VERSION_FIELDS 1 EDEN_RELEASE)
  message(STATUS "Eden Version: ${EDEN_VERSION}-${EDEN_RELEASE}")
else()
  set(EDEN_VERSION "")
  set(EDEN_RELEASE "")
  set(PACKAGE_VERSION "0.0.0")
  message(STATUS "Eden Version: (dev build)")
endif()
string(TIMESTAMP EDEN_BUILD_TIME_UNIX "%s")

# Tell CMake to also look in the directories where getdeps.py installs
# our third-party dependencies.
list(APPEND CMAKE_PREFIX_PATH "${CMAKE_CURRENT_SOURCE_DIR}/external/install")

# CMake include directories
set(CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/CMake"
  # For shipit-transformed builds
  "${CMAKE_CURRENT_SOURCE_DIR}/build/fbcode_builder/CMake"
  ${CMAKE_MODULE_PATH})

include_directories(
  ${CMAKE_SOURCE_DIR}
  ${CMAKE_BINARY_DIR}
)

if (IS_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/external/osxfuse")
  include_directories("${CMAKE_CURRENT_SOURCE_DIR}/external/osxfuse/common")
else()
  find_path(OSXFUSE_INCLUDE_DIR NAMES "fuse_ioctl.h")
  if (OSXFUSE_INCLUDE_DIR)
    include_directories(${OSXFUSE_INCLUDE_DIR})
  endif()
endif()

set(CMAKE_CXX_STANDARD 17)

# Configuration options
set(ENABLE_EDENSCM AUTO CACHE STRING "Enable support for Eden SCM repositories")
set_property(CACHE ENABLE_EDENSCM PROPERTY STRINGS AUTO ON OFF)
set(ENABLE_GIT AUTO CACHE STRING "Enable support for Git repositories")
set_property(CACHE ENABLE_GIT PROPERTY STRINGS AUTO ON OFF)

include(FBCompilerSettings)
include(EdenCompilerSettings)
include(EdenConfigChecks)
include(FBPythonBinary)
include(FBThriftLibrary)
include(RustStaticLibrary)

include_directories(BEFORE ${CMAKE_CURRENT_BINARY_DIR})

set_property(
  DIRECTORY
  ${CMAKE_CURRENT_SOURCE_DIR}
  APPEND
  PROPERTY
  COMPILE_DEFINITIONS
  "FOLLY_XLOG_STRIP_PREFIXES=\"${CMAKE_SOURCE_DIR}:${CMAKE_BINARY_DIR}\""
)
add_subdirectory(eden/fs)
add_subdirectory(eden/integration)
add_subdirectory(eden/scm/lib)
add_subdirectory(eden/test_support)
add_subdirectory(eden/scm/exec/eden_apfs_mount_helper)

if(WIN32)
  # On Windows our Python binaries are compiled into executables which depend
  # on Python.  They need to be able to find the correct Python DLL at runtime.
  # If INSTALL_PYTHON_LIB is enabled we will copy the correct Python DLL into
  # Eden's installation directory so that our programs can easily find the
  # right Python library.
  #
  # It might be nice if we could link against Python statically.  The
  # FindPython3.cmake module does support a Python3_USE_STATIC_LIBS variable,
  # but unfortunately it is ignored on Windows.
  set(
    INSTALL_PYTHON_LIB FALSE CACHE BOOL
    "Install the Python DLL in Eden's bin/ directory."
  )
  if(INSTALL_PYTHON_LIB)
    get_target_property(python_dll Python3::Python LOCATION)
    install(FILES "${python_dll}" DESTINATION bin)
  endif()

  # Also a emit a file listing additional directories containing runtime
  # library dependencies.  On Windows this is needed to help figure out runtime
  # dependencies needed to run the generated build artifacts.  This file can be
  # processed by getdeps.py
  get_filename_component(python_dll_dir "${python_dll}" DIRECTORY)
  file(TO_NATIVE_PATH "${python_dll_dir}" python_dll_dir)
  file(WRITE "${CMAKE_BINARY_DIR}/LIBRARY_DEP_DIRS.txt" "${python_dll_dir}\n")
endif()

configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/CMake/eden-config.h.in
  ${CMAKE_CURRENT_BINARY_DIR}/eden/fs/eden-config.h
)
configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/CMake/eden-config.cpp.in
  ${CMAKE_CURRENT_BINARY_DIR}/eden/fs/eden-config.cpp
)

if(EDEN_HAVE_GIT)
  set(EDEN_HAVE_GIT_PY "True")
else()
  set(EDEN_HAVE_GIT_PY "False")
endif()

if(LINUX)
  set(EDEN_HAVE_NFS_PY "True")
else()
  set(EDEN_HAVE_NFS_PY "False")
endif()

configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/CMake/eden-config.py.in
  ${CMAKE_CURRENT_BINARY_DIR}/eden/fs/py/eden/config.py
)
