file(GLOB STORE_HG_SRCS "*.cpp")
list(REMOVE_ITEM STORE_HG_SRCS ${CMAKE_CURRENT_SOURCE_DIR}/tester.cpp)
add_library(
  eden_store_hg STATIC
    ${STORE_HG_SRCS}
)
target_link_libraries(
  eden_store_hg
  PUBLIC
    eden_model
    eden_store
    eden_utils
)

add_executable(
  hg_store_tester
    tester.cpp
)
target_link_libraries(
  hg_store_tester
    eden_store_hg
)
