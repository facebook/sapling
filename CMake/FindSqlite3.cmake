#
# Find sqlite3
#
# This package sets:
# SQLITE3_FOUND - Whether sqlite3 was found
# SQLITE3_INCLUDE_DIR - The include directory for sqlite3
# SQLITE3_LIBRARY - The sqlite3 library
# 
include(FindPackageHandleStandardArgs)

find_path(SQLITE3_INCLUDE_DIR NAMES sqlite3.h)
find_library(SQLITE3_LIBRARY NAMES sqlite3)
find_package_handle_standard_args(
  SQLITE3
  DEFAULT_MSG
  SQLITE3_INCLUDE_DIR
  SQLITE3_LIBRARY
)
mark_as_advanced(SQLITE3_INCLUDE_DIR SQLITE3_LIBRARY)
