file(GLOB UTILS_SRCS "*.cpp")
add_library(
  eden_utils STATIC
    ${UTILS_SRCS}
)
target_link_libraries(
  eden_utils
  PUBLIC
    eden_service
    Folly::folly
)
