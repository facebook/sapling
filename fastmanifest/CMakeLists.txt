# Copyright 2016-present Facebook. All Rights Reserved.
#
# Build file.

cmake_minimum_required(VERSION 3.4)
project(fastmanifest)

#set(CMAKE_VERBOSE_MAKEFILE on)

SET(CMAKE_C_FLAGS "-Wall -Werror")
SET(CMAKE_C_FLAGS_DEBUG "-O0 -g")
SET(CMAKE_C_FLAGS_RELEASE "-O3 -DNDEBUG")
SET(CMAKE_C_FLAGS_RELWITHDEBINFO "-O0 -g")

add_library(bsearch
        bsearch.c
        bsearch.h
        )

add_executable(bsearch_test bsearch_test.c)
target_link_libraries(bsearch_test bsearch)

add_library(fastmanifest
        checksum.c
        checksum.h
        internal_result.h
        node.c
        result.h
        tree.c
        tree_arena.c
        tree_arena.h
        tree_convert.c
        buffer.h
        buffer.c
        tree_copy.c)
find_library(OPENSSL
        NAME crypto
        PATHS /opt/local/lib)
target_include_directories(fastmanifest PUBLIC /opt/local/include)
target_link_libraries(fastmanifest PUBLIC ${OPENSSL})

SET(TEST_HEADER_FILES tests.h)

add_executable(checksum_test checksum_test.c ${TEST_HEADER_FILES})
target_link_libraries(checksum_test bsearch fastmanifest)

add_executable(node_test node_test.c ${TEST_HEADER_FILES})
target_link_libraries(node_test bsearch fastmanifest)

add_executable(tree_test tree_test.c ${TEST_HEADER_FILES})
target_link_libraries(tree_test bsearch fastmanifest)

add_executable(tree_convert_test tree_convert_test.c ${TEST_HEADER_FILES})
target_link_libraries(tree_convert_test bsearch fastmanifest)

add_executable(tree_copy_test tree_copy_test.c)
target_link_libraries(tree_copy_test bsearch fastmanifest)

add_executable(tree_convert_rt tree_convert_rt.c)
target_link_libraries(tree_convert_rt bsearch fastmanifest)

add_executable(null_test null_test.c)

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
ENDIF(CMAKE_BUILD_TYPE MATCHES RelWithDebInfo OR CMAKE_BUILD_TYPE MATCHES Release)
