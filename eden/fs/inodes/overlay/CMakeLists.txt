add_thrift_cpp2_library(
  eden_overlay_thrift
  overlay.thrift
)

file(GLOB OVERLAY_SRCS "*.cpp")
add_library(
  eden_overlay STATIC
    ${OVERLAY_SRCS}
)
target_link_libraries(
  eden_overlay
  PUBLIC
    eden_overlay_thrift
    eden_fuse
    eden_utils
)
