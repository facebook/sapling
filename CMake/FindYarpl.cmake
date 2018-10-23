#
# - Try to find the Facebook YARPL library
# This will define
# Yarpl_FOUND
# YARPL_INCLUDE_DIR
# YARPL_LIBRARIES
#

find_path(YARPL_INCLUDE_DIRS yarpl/Flowable.h)
find_library(YARPL_LIBRARIES yarpl)
mark_as_advanced(YARPL_INCLUDE_DIRS YARPL_LIBRARIES)

include(FindPackageHandleStandardArgs)
find_package_handle_standard_args(Yarpl YARPL_INCLUDE_DIRS YARPL_LIBRARIES)
