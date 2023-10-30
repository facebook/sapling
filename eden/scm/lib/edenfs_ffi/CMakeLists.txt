# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

find_program(CARGO_COMMAND cargo REQUIRED)
set(crate_name edenfs_ffi)
if(DEFINED ARG_FEATURES)
  set(cargo_flags build
    $<IF:$<CONFIG:Debug>,,--release>
    -p ${crate_name}
    --features ${ARG_FEATURES})
else()
  set(cargo_flags build $<IF:$<CONFIG:Debug>,,--release> -p ${crate_name})
endif()

if (CMAKE_BUILD_TYPE STREQUAL "Debug")
    set(TARGET_DIR "debug")
else ()
    set(TARGET_DIR "release")
endif ()

if(USE_CARGO_VENDOR)
  set(extra_cargo_env "CARGO_HOME=${RUST_CARGO_HOME}")
  set(cargo_flags ${cargo_flags})
endif()

set(CARGO_MANIFEST ${CMAKE_SOURCE_DIR}/eden/scm/lib/edenfs_ffi/Cargo.toml)
set(CARGO_TARGET_DIR ${CMAKE_SOURCE_DIR}/eden/scm/lib/edenfs_ffi/target)

set(FFI_SOURCE_FILE ${CMAKE_SOURCE_DIR}/eden/scm/lib/edenfs_ffi/src/lib.rs)
set(FFI_BRIDGE_CPP ${CARGO_TARGET_DIR}/cxxbridge/edenfs_ffi/src/lib.rs.cc)
set(FFI_LIB ${CARGO_TARGET_DIR}/${TARGET_DIR}/${CMAKE_STATIC_LIBRARY_PREFIX}edenfs_ffi${CMAKE_STATIC_LIBRARY_SUFFIX})

# Add a custom command that builds the rust crate and generates C++ bridge code
add_custom_command(
        OUTPUT ${FFI_BRIDGE_CPP} ${FFI_LIB}
        COMMAND ${extra_cargo_env}
          ${CARGO_COMMAND}
          ${cargo_flags}
          --manifest-path ${CARGO_MANIFEST}
        DEPENDS ${FFI_SOURCE_FILE}
        USES_TERMINAL
        COMMENT "Running cargo..."
)

file(GLOB STORE_FFI_SRCS ${FFI_BRIDGE_CPP})

add_library(
  hg_eden_ffi STATIC
  ${FFI_BRIDGE_CPP}
)

target_include_directories(
        hg_eden_ffi
        PRIVATE
        include/
        ${CARGO_TARGET_DIR}/eden/scm/lib/edenfs_ffi/src/
)

target_link_libraries(hg_eden_ffi PUBLIC
    eden_model
    eden_utils
    ${RE2_LIBRARY}
    ${FFI_LIB})

set_target_properties(
        hg_eden_ffi
        PROPERTIES ADDITIONAL_CLEAN_FILES ${CARGO_TARGET_DIR}
)

# Windows-only configuration
if(WIN32)
    target_link_libraries(hg_eden_ffi userenv ws2_32 bcrypt)
    set_target_properties(
            hg_eden_ffi
            PROPERTIES
            MSVC_RUNTIME_LIBRARY "MultiThreadedDLL"
            RUNTIME_OUTPUT_DIRECTORY_DEBUG ${CMAKE_BINARY_DIR}
            RUNTIME_OUTPUT_DIRECTORY_RELEASE ${CMAKE_BINARY_DIR}
    )
endif()
