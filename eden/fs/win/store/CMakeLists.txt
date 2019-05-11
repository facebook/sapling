file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_win_store STATIC
    ${CONFIG_SRCS}
)

target_link_libraries(
  eden_win_store
  PUBLIC
   Folly::folly
)
