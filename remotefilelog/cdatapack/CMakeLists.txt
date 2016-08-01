# Copyright 2016-present Facebook. All Rights Reserved.
#
# Build file.
#
# no-check-code

cmake_minimum_required(VERSION 3.5)
project(cdatapack)

SET(CMAKE_C_FLAGS "-std=c99 -Wall -Wshorten-64-to-32 -Wincompatible-pointer-types-discards-qualifiers -Werror")
SET(CMAKE_C_FLAGS_DEBUG "-O0 -g")
SET(CMAKE_C_FLAGS_RELEASE "-O3 -DNDEBUG")
SET(CMAKE_C_FLAGS_RELWITHDEBINFO "-O0 -g")

add_library(cdatapack
        buffer.h
        cdatapack.c
        cdatapack.h
        convert.h)

add_executable(cdatapack_dump cdatapack_dump.c)
target_link_libraries(cdatapack_dump cdatapack)

add_executable(cdatapack_get cdatapack_get.c)
target_link_libraries(cdatapack_get cdatapack)

add_executable(null_test null_test.c)
