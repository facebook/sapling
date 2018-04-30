add_thrift_cpp2_library(
  eden_overlay_thrift
  overlay.thrift
)

file(GLOB INODES_SRCS "*.cpp")
add_library(
  eden_inodes STATIC
    ${INODES_SRCS}
)
target_link_libraries(
  eden_inodes
  PUBLIC
    eden_overlay_thrift
    eden_fuse
    eden_journal
    eden_store
    eden_utils
)
