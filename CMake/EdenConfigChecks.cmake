include(FindPkgConfig)

find_package(folly CONFIG REQUIRED)
find_package(wangle CONFIG REQUIRED)
find_package(FBThrift CONFIG REQUIRED)
find_package(GMock MODULE REQUIRED)

find_package(SELinux)
set(EDEN_HAVE_SELINUX SELINUX_FOUND)

find_package(LibGit2 REQUIRED)

# The following packages ship with their own CMake configuration files
find_package(cpptoml CONFIG REQUIRED)
find_package(gflags CONFIG REQUIRED)

# TODO: It shouldn't be too hard to turn RocksDB and sqlite3 into optional
# dependencies, since we have alternate LocalStore implementations.
find_package(RocksDB CONFIG REQUIRED)
set(EDEN_HAVE_ROCKSDB RocksDB_FOUND)
find_package(Sqlite3 REQUIRED)
set(EDEN_HAVE_SQLITE3 SQLITE3_FOUND)

find_package(cpptoml REQUIRED)

# We currently do not have treemanifest support in the opensource build
set(EDEN_HAVE_HG_TREEMANIFEST OFF)
