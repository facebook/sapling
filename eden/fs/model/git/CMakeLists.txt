file(GLOB MODEL_GIT_SRCS "*.cpp")
add_library(
  eden_model_git STATIC
    ${MODEL_GIT_SRCS}
)
target_link_libraries(
  eden_model_git
  PUBLIC
    eden_model
)
