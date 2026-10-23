# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Defines the library for one BUCK cpp_library rule. The calls live in the
# generated_targets.cmake files that eden/facebook/generate_cmake.py writes.
#
#   eden_add_library(<name>
#     [SRCS <source>...]
#     [DEPS <library>...]
#     [EXPORTED_DEPS <library>...]
#   )
#
# A library without sources only carries headers and its EXPORTED_DEPS.
function(eden_add_library name)
  cmake_parse_arguments(ARG "" "" "SRCS;DEPS;EXPORTED_DEPS" ${ARGN})
  if(ARG_SRCS)
    add_library(${name} STATIC ${ARG_SRCS})
    target_link_libraries(
      ${name} PUBLIC ${ARG_EXPORTED_DEPS} PRIVATE ${ARG_DEPS}
    )
  else()
    add_library(${name} INTERFACE)
    target_link_libraries(${name} INTERFACE ${ARG_EXPORTED_DEPS})
  endif()
  set_property(DIRECTORY APPEND PROPERTY EDEN_LIBRARIES ${name})
endfunction()

# Defines <name> as an INTERFACE library over every eden_add_library() target
# in the current directory, for the hand-written tests and binaries that link
# a whole directory.
function(eden_add_directory_library name)
  get_property(libraries DIRECTORY PROPERTY EDEN_LIBRARIES)
  add_library(${name} INTERFACE)
  target_link_libraries(${name} INTERFACE ${libraries})
endfunction()
