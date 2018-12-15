file(GLOB FUSE_SRCS "*.cpp")
add_library(
  eden_fuse STATIC
    ${FUSE_SRCS}
    privhelper/PrivHelper.cpp
    privhelper/PrivHelperConn.cpp
    privhelper/PrivHelperServer.cpp
    privhelper/UserInfo.cpp
)
target_link_libraries(
  eden_fuse
  PUBLIC
    eden_fuse_privhelper
    common_stats
    Folly::folly
)

add_subdirectory(privhelper)
