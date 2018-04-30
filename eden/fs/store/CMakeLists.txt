file(GLOB STORE_SRCS "*.cpp")
add_library(
  eden_store STATIC
    ${STORE_SRCS}
)
target_link_libraries(
  eden_store
  PUBLIC
    eden_model
    eden_model_git
    eden_rocksdb
    eden_service_thrift
    eden_sqlite
)

add_subdirectory(hg)
add_subdirectory(git)
