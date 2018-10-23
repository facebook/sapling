file(GLOB PRIVHELPER_SRCS "*.cpp")
add_library(
  eden_fuse_privhelper STATIC
    ${PRIVHELPER_SRCS}
)

target_include_directories(
  eden_fuse_privhelper
  PRIVATE
    ${SELINUX_INCLUDE_DIR}
)
target_link_libraries(
  eden_fuse_privhelper
  PUBLIC
    eden_utils
    Folly::folly
    ${SELINUX_LIBRARIES}
)
