# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FBCMakeParseArgs)

function(add_fbthrift_rust_library LIB_NAME THRIFT_FILE)
  set(one_value_args NAMESPACE THRIFT_INCLUDE_DIR)
  set(multi_value_args SERVICES DEPENDS OPTIONS)
  fb_cmake_parse_args(
    ARG "" "${one_value_args}" "${multi_value_args}" "${ARGN}"
  )

  if(NOT DEFINED ARG_THRIFT_INCLUDE_DIR)
    set(ARG_THRIFT_INCLUDE_DIR "include/thrift-files")
  endif()

  get_filename_component(base ${THRIFT_FILE} NAME_WE)
  set(output_dir "${CMAKE_CURRENT_BINARY_DIR}/${THRIFT_FILE}-rs")
  set(rust_output_dir "${output_dir}/gen-rust")

  list(APPEND generated_sources
    "${rust_output_dir}/client.rs"
    "${rust_output_dir}/consts.rs"
    "${rust_output_dir}/errors.rs"
    "${rust_output_dir}/server.rs"
    "${rust_output_dir}/services.rs"
    "${rust_output_dir}/types.rs"

    "${rust_output_dir}/mock.rs"

    "${rust_output_dir}/namespace-cpp2"
    "${rust_output_dir}/namespace-rust"
    "${rust_output_dir}/service-names"
  )
  foreach(service IN LISTS ARG_SERVICES)
    list(APPEND generated_sources
      ${rust_output_dir}/${service}.rs
    )
  endforeach()

  # Define a dummy interface library to help propagate the thrift include
  # directories between dependencies.
  add_library("${LIB_NAME}.thrift_includes" INTERFACE)
  target_include_directories(
    "${LIB_NAME}.thrift_includes"
    INTERFACE
      "$<BUILD_INTERFACE:${CMAKE_SOURCE_DIR}>"
      "$<INSTALL_INTERFACE:${ARG_THRIFT_INCLUDE_DIR}>"
  )
  foreach(dep IN LISTS ARG_DEPENDS)
    target_link_libraries(
      "${LIB_NAME}.thrift_includes"
      INTERFACE "${dep}.thrift_includes"
    )
  endforeach()

  # This generator expression gets the list of include directories required
  # for all of our dependencies.
  # It requires using COMMAND_EXPAND_LISTS in the add_custom_command() call
  # below.  COMMAND_EXPAND_LISTS is only available in CMake 3.8+
  # If we really had to support older versions of CMake we would probably need
  # to use a wrapper script around the thrift compiler that could take the
  # include list as a single argument and split it up before invoking the
  # thrift compiler.
  if (NOT POLICY CMP0067)
    message(FATAL_ERROR "add_fbthrift_rust_library() requires CMake 3.8+")
  endif()
  set(
    thrift_include_options
    "-I;$<JOIN:$<TARGET_PROPERTY:${LIB_NAME}.thrift_includes,INTERFACE_INCLUDE_DIRECTORIES>,;-I;>"
  )

  # CMake 3.12 is finally getting a list(JOIN) function, but until then
  # treating the list as a string and replacing the semicolons is good enough.
  string(REPLACE ";" "," GEN_ARG_STR "${ARG_OPTIONS}")

  add_custom_command(
    OUTPUT
      ${generated_sources}
    COMMAND
      "${CMAKE_COMMAND}" -E make_directory "${output_dir}"
    COMMAND
      cargo build --target-dir "${output_dir}/thrift"
        # -E env OUT_DIR="${output_dir}"
        #   "PATH=${CMAKE_CURRENT_BINARY_DIR}/my-bin:$ENV{PATH}"
    WORKING_DIRECTORY
      "${CMAKE_CURRENT_SOURCE_DIR}"
    MAIN_DEPENDENCY
      "${THRIFT_FILE}"
    DEPENDS
      "${FBTHRIFT_COMPILER}"
  )

  add_custom_target("${LIB_NAME}.PHONY" ALL DEPENDS ${generated_sources})
endfunction()
