file(GLOB TRACING_SRCS "*.cpp")
add_library(
  eden_tracing STATIC
    ${TRACING_SRCS}
)
target_link_libraries(
  eden_tracing
  PUBLIC
    eden_utils
    Folly::folly
)
