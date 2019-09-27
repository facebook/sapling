# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

include(FindPkgConfig)

set(CMAKE_THREAD_PREFER_PTHREAD ON)
set(THREADS_PREFER_PTHREAD_FLAG ON)
find_package(Threads REQUIRED)

find_package(gflags CONFIG REQUIRED)
include_directories(${GFLAGS_INCLUDE_DIR})

find_package(glog CONFIG REQUIRED)
include_directories(${GLOG_INCLUDE_DIR})

# We need to probe for libevent because the current stable version
# of libevent doesn't publish the -L libdir in its exported interface
# which means that folly simply exports `event` to us, leaving us
# unable to resolve and link it.  Pulling in the package via its
# config causes the event target to be defined and satisfies the
# linker.
find_package(Libevent CONFIG QUIET)

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

find_package(yarpl CONFIG REQUIRED)
find_package(rsocket CONFIG REQUIRED)

find_package(GMock MODULE REQUIRED)
include_directories(${GMOCK_INCLUDEDIR} ${LIBGMOCK_INCLUDE_DIR})
include(GoogleTest)
enable_testing()

find_package(OpenSSL MODULE REQUIRED)

find_package(SELinux)
set(EDEN_HAVE_SELINUX ${SELINUX_FOUND})

if("${ENABLE_GIT}" STREQUAL "AUTO")
  find_package(LibGit2 MODULE)
  set(EDEN_HAVE_GIT "${LibGit2_FOUND}")
elseif(ENABLE_GIT)
  find_package(LibGit2 MODULE REQUIRED)
  set(EDEN_HAVE_GIT "${LibGit2_FOUND}")
else()
  set(EDEN_HAVE_GIT OFF)
endif()

if("${ENABLE_EDENSCM}" STREQUAL "AUTO")
  find_package(EdenSCM MODULE)
  set(EDEN_HAVE_EDENSCM "${EdenSCM_FOUND}")
  if(NOT EDEN_HAVE_EDENSCM)
    message(STATUS "Building with EdenSCM support disabled")
  endif()
elseif(ENABLE_EDENSCM)
  find_package(EdenSCM MODULE REQUIRED)
  set(EDEN_HAVE_EDENSCM "${EdenSCM_FOUND}")
else()
  set(EDEN_HAVE_EDENSCM OFF)
endif()

if("${EDEN_HAVE_EDENSCM}")
  include(FBMercurialFeatures)
endif()

# The following packages ship with their own CMake configuration files
find_package(cpptoml CONFIG REQUIRED)
find_package(gflags CONFIG REQUIRED)

# TODO: It shouldn't be too hard to turn RocksDB and sqlite3 into optional
# dependencies, since we have alternate LocalStore implementations.
find_package(RocksDB CONFIG REQUIRED)
set(EDEN_HAVE_ROCKSDB ${RocksDB_FOUND})
find_package(Sqlite3 REQUIRED)
set(EDEN_HAVE_SQLITE3 ${SQLITE3_FOUND})

find_package(python-toml REQUIRED)

find_package(CURL)
set(EDEN_HAVE_CURL ${CURL_FOUND})

if (WIN32)
  find_package(Prjfs MODULE REQUIRED)
endif()
set(EDEN_HAVE_RUST_DATAPACK OFF)
set(EDEN_HAVE_MONONOKE OFF)

# TODO(strager): Support systemd in the opensource build.
set(EDEN_HAVE_SYSTEMD OFF)

if (WIN32)
  set(DEFAULT_ETC_EDEN_DIR "C:/tools/eden/config")
else()
  set(DEFAULT_ETC_EDEN_DIR "/etc/eden")
endif()
set(
  ETC_EDEN_DIR "${DEFAULT_ETC_EDEN_DIR}" CACHE STRING
  "The directory for system-wide EdenFS configuration files."
)
