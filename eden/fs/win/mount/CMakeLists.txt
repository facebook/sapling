file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_mount STATIC
    ${CONFIG_SRCS}
)

target_link_libraries(
  eden_win_mount
  PUBLIC
   Folly::folly
)
