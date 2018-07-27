file(GLOB PRIVHELPER_SRCS "*.cpp")
add_library(
  eden_fuse_privhelper STATIC
    ${PRIVHELPER_SRCS}
)

set(PRIVHELPER_INCLUDES)
set(PRIVHELPER_LIBS)
if(SELINUX_FOUND)
  list(APPEND PRIVHELPER_INCLUDES ${SELINUX_INCLUDE_DIR})
  list(APPEND PRIVHELPER_LIBS ${SELINUX_LIBRARIES})
endif(SELINUX_FOUND)

target_include_directories(
  eden_fuse_privhelper
  PRIVATE
    ${PRIVHELPER_INCLUDE_DIR}
)
target_link_libraries(
  eden_fuse_privhelper
  PUBLIC
    eden_utils
    Folly::folly
    ${PRIVHELPER_LIBRARIES}
)
