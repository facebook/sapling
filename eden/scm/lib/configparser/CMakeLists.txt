rust_static_library(rust_configparser CRATE configparser)
install_rust_static_library(
  rust_configparser
  EXPORT mercurial
  INSTALL_DIR lib
)

add_library(configparser ConfigParser.cpp)
set_target_properties(
  configparser
  PROPERTIES
  PUBLIC_HEADER
  ConfigParser.h
)
target_include_directories(configparser PUBLIC
  $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
  $<INSTALL_INTERFACE:include>
)
target_link_libraries(
  configparser
  PRIVATE
  rust_configparser
  Folly::folly
)

install(
  TARGETS configparser
  EXPORT mercurial
  LIBRARY DESTINATION ${LIB_INSTALL_DIR}
  PUBLIC_HEADER DESTINATION "include/scm/hg/lib/configparser/"
)
