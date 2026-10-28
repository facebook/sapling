# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FindPkgConfig)

set(CMAKE_THREAD_PREFER_PTHREAD ON)
set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

find_package(gflags CONFIG REQUIRED)
include_directories(${GFLAGS_INCLUDE_DIR})

find_package(Glog REQUIRED)
include_directories(${GLOG_INCLUDE_DIR})

# We need to probe for libevent because the current stable version
# of libevent doesn't publish the -L libdir in its exported interface
# which means that folly simply exports `event` to us, leaving us
# unable to resolve and link it.  Pulling in the package via its
# config causes the event target to be defined and satisfies the
# linker.
find_package(Libevent CONFIG QUIET)

find_package(Boost 1.69.0 REQUIRED COMPONENTS filesystem)

find_package(fmt CONFIG REQUIRED)
find_package(folly CONFIG REQUIRED)
include_directories(${FOLLY_INCLUDE_DIR})

find_package(fb303 CONFIG REQUIRED)
include_directories(${FB303_INCLUDE_DIR})

find_package(fizz CONFIG REQUIRED)
include_directories(${FIZZ_INCLUDE_DIR})

find_package(wangle CONFIG REQUIRED)
include_directories(${WANGLE_INCLUDE_DIR})

find_package(FBThrift CONFIG REQUIRED COMPONENTS cpp2 py)
include_directories(${FBTHRIFT_INCLUDE_DIR})

find_package(GMock MODULE REQUIRED)
include_directories(${GMOCK_INCLUDEDIR} ${LIBGMOCK_INCLUDE_DIR})
include(GoogleTest)
enable_testing()

find_package(OpenSSL MODULE REQUIRED)

find_package(SELinux)
set(EDEN_HAVE_SELINUX ${SELINUX_FOUND})

if(NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
  set(EDEN_HAVE_GIT OFF)
elseif("${ENABLE_GIT}" STREQUAL "AUTO")
  find_package(LibGit2 MODULE)
  set(EDEN_HAVE_GIT "${LibGit2_FOUND}")
elseif(ENABLE_GIT)
  find_package(LibGit2 MODULE REQUIRED)
  set(EDEN_HAVE_GIT "${LibGit2_FOUND}")
else()
  set(EDEN_HAVE_GIT OFF)
endif()

find_package(Re2 MODULE REQUIRED)
include_directories(${RE2_INCLUDE_DIR})

find_package(edencommon CONFIG REQUIRED)

# The following packages ship with their own CMake configuration files
find_package(cpptoml CONFIG REQUIRED)
find_package(gflags CONFIG REQUIRED)

find_package(BLAKE3 REQUIRED CONFIG)
include_directories(${BLAKE3_INCLUDE_DIR})

find_package(python-psutil REQUIRED)

# The Python CLI uses thrift-python, which getdeps only builds on Linux, and
# the thrift-python modules that fb303 builds with FB303_THRIFT_PYTHON.
if (ENABLE_THRIFT_PYTHON AND CMAKE_SYSTEM_NAME STREQUAL "Linux")
  include(FBThriftPythonLibrary)
  if (ENABLE_THRIFT_PYTHON STREQUAL "AUTO")
    fb_find_thrift_python_runtime(QUIET)
  else()
    fb_find_thrift_python_runtime()
  endif()
endif()
set(eden_no_python_cli "")
if (NOT ENABLE_THRIFT_PYTHON)
  set(eden_no_python_cli "ENABLE_THRIFT_PYTHON is OFF")
elseif (NOT (THRIFT_PYTHON_SITE_PACKAGES AND FOLLY_PYTHON_SITE_PACKAGES))
  set(eden_no_python_cli "thrift-python not found")
elseif (NOT TARGET fb303::fb303_core_thrift_python.py_lib)
  set(eden_no_python_cli "fb303 was built without FB303_THRIFT_PYTHON")
else()
  find_package(python-toml QUIET)
  find_package(python-filelock QUIET)
  if (NOT (python-toml_FOUND AND python-filelock_FOUND))
    set(eden_no_python_cli
      "getdeps' python-toml and python-filelock packages were not found")
  endif()
endif()
if (eden_no_python_cli)
  set(eden_no_python_cli
    "${eden_no_python_cli}, so the Python CLI that edenfsctl hands "
    "commands like clone, start and stop to is not built")
  if (ENABLE_THRIFT_PYTHON AND NOT ENABLE_THRIFT_PYTHON STREQUAL "AUTO")
    message(FATAL_ERROR ${eden_no_python_cli})
  endif()
  message(STATUS ${eden_no_python_cli})
else()
  set(EDEN_HAVE_THRIFT_PYTHON ON)
endif()

# pexpect is used by some of the integration tests.
# If we don't find it we simply won't run those tests.
find_package(pexpect)

if (NOT WIN32)
  find_package(CURL REQUIRED)
  # The static libcurl that getdeps builds also needs nghttp2.
  find_library(NGHTTP2_LIBRARY nghttp2)
  if(NGHTTP2_LIBRARY AND CURL_LIBRARY MATCHES "\\.a$")
    set_property(
      TARGET CURL::libcurl
      APPEND PROPERTY INTERFACE_LINK_LIBRARIES "${NGHTTP2_LIBRARY}"
    )
  endif()
  find_library(SYSTEMD_LIBRARY systemd)
  if(SYSTEMD_LIBRARY)
    set(SYSTEMD_LIBRARIES "${SYSTEMD_LIBRARY}")
  endif()
endif()

if (WIN32)
  find_package(Prjfs MODULE REQUIRED)
endif()

if (WIN32)
  set(DEFAULT_ETC_EDEN_DIR "C:/ProgramData/Facebook/eden")
else()
  set(DEFAULT_ETC_EDEN_DIR "/etc/eden")
endif()
set(
  ETC_EDEN_DIR "${DEFAULT_ETC_EDEN_DIR}" CACHE STRING
  "The directory for system-wide EdenFS configuration files."
)
