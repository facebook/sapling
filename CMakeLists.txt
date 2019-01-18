cmake_minimum_required(VERSION 3.1.3 FATAL_ERROR)
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
set(CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/CMake" ${CMAKE_MODULE_PATH})
include_directories(
  ${CMAKE_SOURCE_DIR}
  ${CMAKE_BINARY_DIR}
)

if (IS_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}/external/osxfuse")
  include_directories("${CMAKE_CURRENT_SOURCE_DIR}/external/osxfuse/common")
endif()

set(CMAKE_CXX_STANDARD 17)
include(CompilerSettingsUnix)
include(EdenConfigChecks)
include(ThriftCppLibrary)
include(FBMercurialFeatures)

include_directories(BEFORE ${CMAKE_CURRENT_BINARY_DIR})

add_subdirectory(common)
add_subdirectory(eden/fs)

configure_file(
  ${CMAKE_CURRENT_SOURCE_DIR}/CMake/eden-config.h.in
  ${CMAKE_CURRENT_BINARY_DIR}/eden/fs/eden-config.h
)

