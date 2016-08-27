# Copyright 2016-present Facebook. All Rights Reserved.
#
# Build file.
#
# no-check-code

cmake_minimum_required(VERSION 3.5)
project(remotefilelog)

find_package(PythonLibs)

#set(CMAKE_VERBOSE_MAKEFILE on)

SET(CMAKE_C_FLAGS "-std=c99 -Wall -Wshorten-64-to-32 -Wincompatible-pointer-types-discards-qualifiers -Werror")
SET(CMAKE_C_FLAGS_DEBUG "-O0 -g")
SET(CMAKE_C_FLAGS_RELEASE "-O3 -DNDEBUG")
SET(CMAKE_C_FLAGS_RELWITHDEBINFO "-O0 -g")

find_library(OPENSSL
        NAME crypto
        PATHS /opt/local/lib)

# find_library can't seem to come to terms with searching for lz4 on my
# laptop.  it insists on returning /usr/lib/libl.a, which makes no sense to me.
#
# after wasting a bunch of time debugging this, I gave up because nobody else
# uses CLion to build this. :)
SET(LZ4 "/opt/local/lib/liblz4.a")
#find_library(LZ4 NAMES lz4
#             PATHS /opt/local/lib)
#MESSAGE( STATUS "WTF LZ4: " ${LZ4} )

##########################################
# START OF DEFINITIONS FOR CFASTMANIFEST
##########################################

add_library(bsearch
        cfastmanifest/bsearch.c
        cfastmanifest/bsearch.h
        )

add_executable(bsearch_test cfastmanifest/bsearch_test.c)
target_link_libraries(bsearch_test bsearch)

add_library(fastmanifest
        clib/buffer.c
        clib/buffer.h
        cfastmanifest/checksum.c
        cfastmanifest/checksum.h
        clib/convert.h
        cfastmanifest/internal_result.h
        cfastmanifest/node.c
        cfastmanifest/path_buffer.h
        cfastmanifest/result.h
        cfastmanifest/tree.c
        cfastmanifest/tree.h
        cfastmanifest/tree_arena.c
        cfastmanifest/tree_arena.h
        cfastmanifest/tree_convert.c
        cfastmanifest/tree_copy.c
        cfastmanifest/tree_diff.c
        cfastmanifest/tree_disk.c
        cfastmanifest/tree_iterator.c
        cfastmanifest/tree_iterator.h
        cfastmanifest/tree_path.c
        cfastmanifest/tree_path.h
        )

target_include_directories(fastmanifest PUBLIC /opt/local/include clib)
target_link_libraries(fastmanifest PUBLIC ${OPENSSL})

SET(TEST_FILES
        cfastmanifest/tests.c cfastmanifest/tests.h)

add_executable(checksum_test cfastmanifest/checksum_test.c ${TEST_FILES})
target_link_libraries(checksum_test bsearch fastmanifest)

add_executable(node_test cfastmanifest/node_test.c ${TEST_FILES})
target_link_libraries(node_test bsearch fastmanifest)

add_executable(tree_test cfastmanifest/tree_test.c ${TEST_FILES})
target_link_libraries(tree_test bsearch fastmanifest)

add_executable(tree_convert_test cfastmanifest/tree_convert_test.c ${TEST_FILES})
target_link_libraries(tree_convert_test bsearch fastmanifest)

add_executable(tree_copy_test cfastmanifest/tree_copy_test.c ${TEST_FILES})
target_link_libraries(tree_copy_test bsearch fastmanifest)

add_executable(tree_diff_test cfastmanifest/tree_diff_test.c)
target_link_libraries(tree_diff_test bsearch fastmanifest)

add_executable(tree_disk_test cfastmanifest/tree_disk_test.c)
target_link_libraries(tree_disk_test bsearch fastmanifest)

add_executable(tree_iterator_test cfastmanifest/tree_iterator_test.c)
target_link_libraries(tree_iterator_test bsearch fastmanifest)

add_executable(tree_convert_rt cfastmanifest/tree_convert_rt.c)
target_link_libraries(tree_convert_rt bsearch fastmanifest)

add_executable(tree_dump cfastmanifest/tree_dump.c)
target_link_libraries(tree_dump bsearch fastmanifest)

add_executable(tree_iterate_rt cfastmanifest/tree_iterate_rt.c)
target_link_libraries(tree_iterate_rt bsearch fastmanifest)

add_executable(null_test clib/null_test.c)

# If we're making any sort of a release, tie the success of building to
# whether or not unit tests pass.  Obviously, a strict Release build will not
# have assert.h enabled, but the ASSERTs in the unit tests themselves will
# still fire.
IF(CMAKE_BUILD_TYPE MATCHES RelWithDebInfo OR CMAKE_BUILD_TYPE MATCHES Release)
    add_custom_command(
            TARGET bsearch_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:bsearch_test>)

    add_custom_command(
            TARGET checksum_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:checksum_test>)

    add_custom_command(
            TARGET node_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:node_test>)

    add_custom_command(
            TARGET tree_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_test>)

    add_custom_command(
            TARGET tree_convert_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_convert_test>)

    add_custom_command(
            TARGET tree_copy_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_copy_test>)

    add_custom_command(
            TARGET tree_disk_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_disk_test>)

    add_custom_command(
            TARGET tree_diff_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_diff_test>)

    add_custom_command(
            TARGET tree_iterator_test
            POST_BUILD
            COMMAND valgrind -q --error-exitcode=127 $<TARGET_FILE:tree_iterator_test>)
ENDIF(CMAKE_BUILD_TYPE MATCHES RelWithDebInfo OR CMAKE_BUILD_TYPE MATCHES Release)

##########################################
# START OF DEFINITIONS FOR CDATAPACK
##########################################

add_library(cdatapack
        SHARED
        remotefilelog/cdatapack/py-cdatapack.c)

target_link_libraries(cdatapack
        PUBLIC cdatapack_lib)
target_include_directories(cdatapack PUBLIC ${PYTHON_INCLUDE_DIRS})
target_link_libraries(cdatapack PUBLIC ${PYTHON_LIBRARIES})

add_library(cdatapack_lib
        clib/buffer.h
        remotefilelog/cdatapack/cdatapack.c
        remotefilelog/cdatapack/cdatapack.h
        clib/convert.h)

target_include_directories(cdatapack_lib PUBLIC /opt/local/include clib)
target_link_libraries(cdatapack_lib PUBLIC ${LZ4})

add_executable(cdatapack_dump remotefilelog/cdatapack/cdatapack_dump.c)
target_link_libraries(cdatapack_dump cdatapack_lib)

add_executable(cdatapack_get remotefilelog/cdatapack/cdatapack_get.c)
target_link_libraries(cdatapack_get cdatapack_lib)

target_include_directories(cdatapack_get PUBLIC /opt/local/include)
target_link_libraries(cdatapack_get ${OPENSSL})

##########################################
# START OF DEFINITIONS FOR CTREEMANIFEST
##########################################

add_library(ctreemanifest
        SHARED
        remotefilelog/ctreemanifest/convert.h
        remotefilelog/ctreemanifest/manifest.cpp
        remotefilelog/ctreemanifest/manifest.h
        remotefilelog/ctreemanifest/manifest_entry.cpp
        remotefilelog/ctreemanifest/manifest_entry.h
        remotefilelog/ctreemanifest/manifest_fetcher.cpp
        remotefilelog/ctreemanifest/manifest_fetcher.h
        remotefilelog/ctreemanifest/pythonutil.cpp
        remotefilelog/ctreemanifest/pythonutil.h
        remotefilelog/ctreemanifest/py-treemanifest.cpp
        remotefilelog/ctreemanifest/treemanifest.cpp
        remotefilelog/ctreemanifest/treemanifest.h
        )

target_include_directories(ctreemanifest PUBLIC ${PYTHON_INCLUDE_DIRS})
target_link_libraries(ctreemanifest PUBLIC ${PYTHON_LIBRARIES})
