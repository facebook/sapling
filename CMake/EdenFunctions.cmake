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

# Defines the test executable for one BUCK cpp_unittest rule and registers its
# tests with CTest. Like eden_add_library(), the calls live in
# generated_targets.cmake files.
#
#   eden_add_test(<name>
#     [SRCS <source>...]
#     [DEPS <library>...]
#     [ENVIRONMENT <NAME=VALUE>...]
#   )
function(eden_add_test name)
  cmake_parse_arguments(ARG "" "" "SRCS;DEPS;ENVIRONMENT" ${ARGN})
  add_executable(${name} ${ARG_SRCS})
  # cpp_unittest links GTest and a main() that initializes folly whether the
  # rule lists them or not.
  target_link_libraries(
    ${name} PRIVATE ${ARG_DEPS} Folly::folly_test_main ${LIBGMOCK_LIBRARIES}
  )
  if(ARG_ENVIRONMENT)
    gtest_discover_tests(${name} PROPERTIES ENVIRONMENT "${ARG_ENVIRONMENT}")
  else()
    gtest_discover_tests(${name})
  endif()
endfunction()

# Defines <name> as an INTERFACE library over every eden_add_library() target
# in the current directory, for the hand-written binaries that link
# a whole directory.
function(eden_add_directory_library name)
  get_property(libraries DIRECTORY PROPERTY EDEN_LIBRARIES)
  add_library(${name} INTERFACE)
  target_link_libraries(${name} INTERFACE ${libraries})
endfunction()
