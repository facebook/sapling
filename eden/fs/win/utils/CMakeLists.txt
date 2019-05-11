file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_utils STATIC
    ${CONFIG_SRCS}
)

target_link_libraries(
  eden_win_utils
  PUBLIC
    Userenv.lib
    Folly::folly
)
