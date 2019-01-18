# FIXME: need to pull in proxygen for this
file(GLOB STORE_MONONOKE_SRCS "*.cpp")
add_library(
  eden_store_mononoke STATIC
    ${STORE_MONONOKE_SRCS}
)
target_link_libraries(
  eden_store_mononoke
  PUBLIC
    eden_model
    eden_store
    eden_utils
)
