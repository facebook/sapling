file(GLOB SQLITE_SRCS "*.cpp")
add_library(
  eden_sqlite STATIC
    ${SQLITE_SRCS}
)
target_include_directories(
  eden_sqlite
  PUBLIC
    ${SQLITE3_INCLUDE_DIR}
)
target_link_libraries(
  eden_sqlite
  PUBLIC
    Folly::folly
    ${SQLITE3_LIBRARY}
)
