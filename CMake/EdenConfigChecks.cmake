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

if("${ENABLE_GIT}" STREQUAL "AUTO")
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

# This is rather gross.  Eden doesn't directly depend upon Snappy but does so
# indirectly from both folly and rocksdb.
# Rocksdb has some custom logic to find snappy but unfortunately exports a synthesized
# target named `snappy::snappy` instead of using the CONFIG provided by snappy itself,
# and also does not export a way to resolve its own custom definition.
# We use the `Snappy::snappy` (note that the first `S` is uppercase!) exported by
# snappy and synthesize our own `snappy::snappy` to satisfy the linkage.
# Even though we tend to build RelWithDebInfo we need to allow this to work for the
# other common cmake build modes.
# This section of logic can be removed once we've fixed up the behavior in RocksDB.

find_package(Snappy CONFIG REQUIRED)
get_target_property(SNAPPY_LIBS_RELWITHDEBINFO Snappy::snappy IMPORTED_LOCATION_RELWITHDEBINFO)
get_target_property(SNAPPY_LIBS_RELEASE Snappy::snappy IMPORTED_LOCATION_RELEASE)
get_target_property(SNAPPY_LIBS_DEBUG Snappy::snappy IMPORTED_LOCATION_DEBUG)
get_target_property(SNAPPY_INCLUDES Snappy::snappy INTERFACE_INCLUDE_DIRECTORIES)
add_library(snappy::snappy UNKNOWN IMPORTED)
set_target_properties(snappy::snappy PROPERTIES
  IMPORTED_LINK_INTERFACE_LANGUAGES "C"
  IMPORTED_LOCATION_RELWITHDEBINFO "${SNAPPY_LIBS_RELWITHDEBINFO}"
  IMPORTED_LOCATION_RELEASE "${SNAPPY_LIBS_RELEASE}"
  IMPORTED_LOCATION_DEBUG "${SNAPPY_LIBS_DEBUG}"
  INTERFACE_INCLUDE_DIRECTORIES "${SNAPPY_INCLUDES}"
)

# TODO: It shouldn't be too hard to turn RocksDB and sqlite3 into optional
# dependencies, since we have alternate LocalStore implementations.
find_package(RocksDB CONFIG REQUIRED)
set(EDEN_HAVE_ROCKSDB ${RocksDB_FOUND})
find_package(Sqlite3 REQUIRED)
set(EDEN_HAVE_SQLITE3 ${SQLITE3_FOUND})

find_package(python-toml REQUIRED)
find_package(python-filelock REQUIRED)

# pexpect is used by some of the integration tests.
# If we don't find it we simply won't run those tests.
find_package(pexpect)

if (NOT WIN32)
  find_package(CURL REQUIRED)
endif()

if (WIN32)
  find_package(Prjfs MODULE REQUIRED)
endif()

if (
    "${CMAKE_SYSTEM_NAME}" STREQUAL "Linux" AND
    EXISTS "${CMAKE_SOURCE_DIR}/eden/fs/service/facebook/CMakeLists.txt"
)
  set(EDEN_HAVE_USAGE_SERVICE ON)
else()
  set(EDEN_HAVE_USAGE_SERVICE OFF)
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
