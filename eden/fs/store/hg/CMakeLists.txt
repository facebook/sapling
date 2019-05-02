file(GLOB STORE_HG_SRCS "*.cpp")
list(REMOVE_ITEM STORE_HG_SRCS ${CMAKE_CURRENT_SOURCE_DIR}/tester.cpp)
add_library(
  eden_store_hg STATIC
    ${STORE_HG_SRCS}
)

list(APPEND EDEN_STORE_HG_OPTIONAL_DEPS datapack)
list(APPEND EDEN_STORE_HG_OPTIONAL_DEPS eden_store_mononoke)
list(APPEND EDEN_STORE_HG_OPTIONAL_DEPS eden_config)

target_link_libraries(
  eden_store_hg
  PUBLIC
    eden_model
    eden_store
    eden_utils
    ${EDEN_STORE_HG_OPTIONAL_DEPS}
)

add_executable(
  hg_store_tester
    tester.cpp
)
target_link_libraries(
  hg_store_tester
    eden_store_hg
)
