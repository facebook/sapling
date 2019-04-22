add_thrift_cpp2_library(
  eden_service_thrift
  eden.thrift
  SERVICES
    EdenService
  DEPENDS
    fb303_thrift_cpp2
)
add_thrift_cpp2_library(
  streamingeden_thrift
  streamingeden.thrift
  SERVICES
    StreamingEdenService
  DEPENDS
    eden_service_thrift
)

file(GLOB SERVICE_SRCS "*.cpp")
list(REMOVE_ITEM SERVICE_SRCS "${CMAKE_CURRENT_SOURCE_DIR}/eden_gc.cpp")
add_library(
  eden_service STATIC
    ${SERVICE_SRCS}
    oss/main.cpp
)
target_link_libraries(
  eden_service
  PUBLIC
    eden_service_thrift
    streamingeden_thrift
    eden_config
    eden_fuse
    eden_inodes
    eden_model
    eden_model_git
    eden_store_hg
    eden_store_git
    eden_takeover
    eden_tracing
    ${YARPL_LIBRARIES}
    Folly::folly
)

if (CURL_FOUND)
  target_link_libraries(
    eden_service
    PUBLIC
      curl
  )
endif()

if (APPLE)
  target_link_libraries(
    eden_service
    PUBLIC
    "-framework CoreFoundation"
    "-framework IOKit"
  )
endif()
