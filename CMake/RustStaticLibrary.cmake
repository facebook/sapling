include(FBCMakeParseArgs)

# This function creates an interface library target based on the static library
# built by Cargo. It will call Cargo to build a staticlib and generate a CMake
# interface library with it.
#
# This function requires `find_package(Python COMPONENTS Interpreter)`.
#
# You need to set `lib:crate-type = ["staticlib"]` in your Cargo.toml to make
# Cargo build static library.
#
# ```cmake
# rust_static_library(<TARGET> [CRATE <CRATE_NAME>])
# ```
#
# Parameters:
# - TARGET:
#   Name of the target name. This function will create an interface library
#   target with this name.
# - CRATE_NAME:
#   Name of the crate. This parameter is optional. If unspecified, it will
#   fallback to `${TARGET}`.
#
# This function creates two targets:
# - "${TARGET}": an interface library target contains the static library built
#   from Cargo.
# - "${TARGET}.cargo": an internal custom target that invokes Cargo.
#
# If you are going to use this static library from C/C++, you will need to
# write header files for the library (or generate with cbindgen) and bind these
# headers with the interface library.
#
function(rust_static_library TARGET)
  fb_cmake_parse_args(ARG "" "CRATE" "" "${ARGN}")

  if(DEFINED ARG_CRATE)
    set(crate_name "${ARG_CRATE}")
  else()
    set(crate_name "${TARGET}")
  endif()

  set(cargo_target "${TARGET}.cargo")
  set(target_dir $<IF:$<CONFIG:Debug>,debug,release>)
  set(cargo_cmd cargo build $<IF:$<CONFIG:Debug>,,--release> -p ${crate_name})
  set(staticlib_name "${CMAKE_STATIC_LIBRARY_PREFIX}${crate_name}${CMAKE_STATIC_LIBRARY_SUFFIX}")
  set(rust_staticlib "${CMAKE_CURRENT_BINARY_DIR}/${target_dir}/${staticlib_name}")

  if(USE_CARGO_VENDOR)
    # setup_cargo_vendor.py is present only in the internal FB version of this repo,
    # and is used to avoid downloading crates directly from the internet each time
    # our CI runs a build.
    set(pre_cargo_cmd ${Python_EXECUTABLE} ${PROJECT_SOURCE_DIR}/CMake/facebook/setup_cargo_vendor.py)
  else()
    set(pre_cargo_cmd "${CMAKE_COMMAND} -E echo NOP")
  endif()

  if(WIN32)
    # compatibility for cmd.exe
    set(
      cargo_cmd
      set CARGO_TARGET_DIR=${CMAKE_CURRENT_BINARY_DIR}&& ${cargo_cmd}
    )
  else()
    set(
      cargo_cmd
      CARGO_TARGET_DIR=${CMAKE_CURRENT_BINARY_DIR} ${cargo_cmd}
    )
  endif()

  add_custom_target(
    ${cargo_target} ALL
    COMMAND ${pre_cargo_cmd}
    COMMAND ${cargo_cmd}
    COMMENT "Building Rust crate '${crate_name}'..."
    WORKING_DIRECTORY ${CMAKE_CURRENT_SOURCE_DIR}
  )

  add_library(${TARGET} INTERFACE)
  add_dependencies(${TARGET} ${cargo_target})
  set_target_properties(
    ${TARGET}
    PROPERTIES
      INTERFACE_STATICLIB_OUTPUT_PATH "${rust_staticlib}"
      INTERFACE_INSTALL_LIBNAME
        "${CMAKE_STATIC_LIBRARY_PREFIX}${crate_name}_rs${CMAKE_STATIC_LIBRARY_SUFFIX}"
  )
  target_link_libraries(
    ${TARGET}
    INTERFACE "$<BUILD_INTERFACE:${rust_staticlib}>"
  )
endfunction()

# This function installs the interface target generated from the function
# `rust_static_library`. Use this function if you want to export your Rust
# target to external CMake targets.
#
# ```cmake
# install_rust_static_library(
#   <TARGET>
#   INSTALL_DIR <INSTALL_DIR>
#   [EXPORT <EXPORT_NAME>]
# )
# ```
#
# Parameters:
# - TARGET: Name of the Rust static library target.
# - EXPORT_NAME: Name of the exported target.
# - INSTALL_DIR: Path to the directory where this library will be installed.
#
function(install_rust_static_library TARGET)
  fb_cmake_parse_args(ARG "" "EXPORT;INSTALL_DIR" "" "${ARGN}")

  get_property(
    staticlib_output_path
    TARGET "${TARGET}"
    PROPERTY INTERFACE_STATICLIB_OUTPUT_PATH
  )
  get_property(
    staticlib_output_name
    TARGET "${TARGET}"
    PROPERTY INTERFACE_INSTALL_LIBNAME
  )

  if(NOT DEFINED staticlib_output_path)
    message(FATAL_ERROR "Not a rust_static_library target.")
  endif()

  if(NOT DEFINED ARG_INSTALL_DIR)
    message(FATAL_ERROR "Missing required argument.")
  endif()

  if(DEFINED ARG_EXPORT)
    set(install_export_args EXPORT "${ARG_EXPORT}")
  endif()

  set(install_interface_dir "${ARG_INSTALL_DIR}")
  if(NOT IS_ABSOLUTE "${install_interface_dir}")
    set(install_interface_dir "\${_IMPORT_PREFIX}/${install_interface_dir}")
  endif()

  target_link_libraries(
    ${TARGET} INTERFACE
    "$<INSTALL_INTERFACE:${install_interface_dir}/${staticlib_output_name}>"
  )
  install(
    TARGETS ${TARGET}
    ${install_export_args}
    LIBRARY DESTINATION ${ARG_INSTALL_DIR}
  )
  install(
    FILES ${staticlib_output_path}
    RENAME ${staticlib_output_name}
    DESTINATION ${ARG_INSTALL_DIR}
  )
endfunction()
