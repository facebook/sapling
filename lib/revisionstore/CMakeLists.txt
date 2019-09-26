rust_static_library(rust_revisionstore CRATE revisionstore)
install_rust_static_library(
  rust_revisionstore
  EXPORT mercurial
  INSTALL_DIR lib
)

add_library(revisionstore RevisionStore.cpp)
set_target_properties(
  revisionstore
  PROPERTIES
  PUBLIC_HEADER
  RevisionStore.h
)
target_include_directories(revisionstore PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  revisionstore
  PRIVATE
  rust_revisionstore
  Folly::folly
)

install(
  TARGETS revisionstore
  EXPORT mercurial
  LIBRARY DESTINATION ${LIB_INSTALL_DIR}
  PUBLIC_HEADER DESTINATION "include/scm/hg/lib/revisionstore/"
)
