# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

if (PYTHON_EXTENSIONS)
  set(FB303_LANG cpp py)
else ()
  set(FB303_LANG cpp)
endif ()

add_fbthrift_library(
  fb303_thrift
  fb303_core.thrift
  LANGUAGES ${FB303_LANG}
  THRIFT_INCLUDE_DIR "${INCLUDE_INSTALL_DIR}/thrift-files"
  SERVICES
    BaseService
)
if (BUILD_SHARED_LIBS)
  set_property(TARGET fb303_thrift_cpp PROPERTY VERSION ${PACKAGE_VERSION})
endif ()

install(
  TARGETS fb303_thrift_cpp
  EXPORT fb303-exports
  LIBRARY DESTINATION "${LIB_INSTALL_DIR}"
  ARCHIVE DESTINATION "${LIB_INSTALL_DIR}"
  PUBLIC_HEADER DESTINATION
    "$<TARGET_PROPERTY:fb303_thrift_cpp,HEADER_INSTALL_DIR>"
)
install(
  TARGETS fb303_thrift_cpp.thrift_includes
  EXPORT fb303-exports
)
install(
  FILES fb303_core.thrift
  DESTINATION "$<TARGET_PROPERTY:fb303_thrift_cpp,THRIFT_INSTALL_DIR>"
)

if (PYTHON_EXTENSIONS)
  install_fb_python_library(fb303_thrift_py EXPORT fb303-exports)
  install(
    TARGETS fb303_thrift_py.thrift_includes
    EXPORT fb303-exports
  )
endif ()
