file(GLOB CONFIG_SRCS "*.cpp")
add_library(
  eden_config STATIC
    ${CONFIG_SRCS}
)
target_link_libraries(
  eden_config
  PUBLIC
    eden_utils
)
target_include_directories(
  eden_config
  PRIVATE
    ${CPPTOML_INCLUDE_DIR}
)
