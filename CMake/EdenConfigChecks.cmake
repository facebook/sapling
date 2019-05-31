# Copyright (c) 2016-present, Facebook, Inc.
# All rights reserved.
#
# This source code is licensed under the BSD-style license found in the
# LICENSE file in the root directory of this source tree. An additional grant
# of patent rights can be found in the PATENTS file in the same directory.

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

find_package(fizz CONFIG REQUIRED)
include_directories(${FIZZ_INCLUDE_DIR})

find_package(wangle CONFIG REQUIRED)
include_directories(${WANGLE_INCLUDE_DIR})

find_package(FBThrift CONFIG REQUIRED)
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

find_package(LibGit2 REQUIRED)
include_directories(${LIBGIT2_INCLUDE_DIRS})

# The following packages ship with their own CMake configuration files
find_package(cpptoml CONFIG REQUIRED)
find_package(gflags CONFIG REQUIRED)

# TODO: It shouldn't be too hard to turn RocksDB and sqlite3 into optional
# dependencies, since we have alternate LocalStore implementations.
find_package(RocksDB CONFIG REQUIRED)
set(EDEN_HAVE_ROCKSDB ${RocksDB_FOUND})
find_package(Sqlite3 REQUIRED)
set(EDEN_HAVE_SQLITE3 ${SQLITE3_FOUND})

find_package(cpptoml REQUIRED)

find_package(CURL)
set(EDEN_HAVE_CURL ${CURL_FOUND})

if (WIN32)
  find_package(Prjfs MODULE REQUIRED)
endif()
set(EDEN_WIN_NO_RUST_DATAPACK ON)
set(EDEN_WIN_NOMONONOKE ON)

# TODO(strager): Support systemd in the opensource build.
set(EDEN_HAVE_SYSTEMD OFF)
