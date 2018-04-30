file(GLOB JOURNAL_SRCS "*.cpp")
add_library(
  eden_journal STATIC
    ${JOURNAL_SRCS}
)
target_link_libraries(
  eden_journal
  PUBLIC
    eden_model
    eden_utils
)
