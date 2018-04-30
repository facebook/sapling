#
# Find cpptoml
#
# This package sets:
# CPPTOML_FOUND - Whether cpptoml was found
# CPPTOML_INCLUDE_DIR - The include directory for cpptoml
# cpptoml is a header-only library, so there is no CPPTOML_LIBRARY variable.
# 
include(FindPackageHandleStandardArgs)

find_path(CPPTOML_INCLUDE_DIR NAMES cpptoml.h)
find_package_handle_standard_args(
  CPPTOML
  DEFAULT_MSG
  CPPTOML_INCLUDE_DIR
)
mark_as_advanced(CPPTOML_INCLUDE_DIR)
