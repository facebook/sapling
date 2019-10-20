rust_static_library(rust_backingstore CRATE backingstore)
install_rust_static_library(
  rust_backingstore
  EXPORT mercurial
  INSTALL_DIR lib
)

file(GLOB C_API_SRCS "c_api/*.cpp")
add_library(backingstore "${C_API_SRCS}")
set_target_properties(
  backingstore
  PROPERTIES
  PUBLIC_HEADER
  "c_api/HgNativeBackingStore.h;c_api/RustBackingStore.h"
)
target_include_directories(backingstore PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  backingstore
  PRIVATE
  rust_backingstore
  Folly::folly
)

# curl used in the Rust crate has its own copy of curl compiled and it uses
# Crypt32 and Secur32 on Windows. We need to declare the link dependencies here
# to avoid linker errors.
if (WIN32)
  target_link_libraries(
    backingstore
    PRIVATE
    Crypt32
    Secur32
    Ncrypt
  )
endif()

install(
  TARGETS backingstore
  EXPORT mercurial
  LIBRARY DESTINATION ${LIB_INSTALL_DIR}
  PUBLIC_HEADER DESTINATION "include/scm/hg/lib/backingstore/c_api"
)
