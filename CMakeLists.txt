# Copyright (c) Facebook, Inc. and its affiliates.
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

# Tell CMake to also look in the directories where getdeps.py installs
# our third-party dependencies.
list(APPEND CMAKE_PREFIX_PATH "${CMAKE_CURRENT_SOURCE_DIR}/external/install")

# CMake include directories
set(CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/CMake"
  # Eventually the files in eden/scm/CMake/ should be moved to CMake/
  "${CMAKE_CURRENT_SOURCE_DIR}/eden/scm/CMake"
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

if (NOT WIN32)
  include(CompilerSettingsUnix)
endif()
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
add_subdirectory(eden/cli)
add_subdirectory(eden/fs)
add_subdirectory(eden/integration)
add_subdirectory(eden/py)
add_subdirectory(eden/scm/lib)
add_subdirectory(eden/test_support)

configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/CMake/eden-config.h.in
  ${CMAKE_CURRENT_BINARY_DIR}/eden/fs/eden-config.h
)
