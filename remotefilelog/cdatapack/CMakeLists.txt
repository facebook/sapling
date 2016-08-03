# Copyright 2016-present Facebook. All Rights Reserved.
#
# Build file.
#
# no-check-code

cmake_minimum_required(VERSION 3.5)
project(cdatapack)

#set(CMAKE_VERBOSE_MAKEFILE on)

SET(CMAKE_C_FLAGS "-std=c99 -Wall -Wshorten-64-to-32 -Wincompatible-pointer-types-discards-qualifiers -Werror")
SET(CMAKE_C_FLAGS_DEBUG "-O0 -g")
SET(CMAKE_C_FLAGS_RELEASE "-O3 -DNDEBUG")
SET(CMAKE_C_FLAGS_RELWITHDEBINFO "-O0 -g")

add_library(cdatapack
        buffer.h
        cdatapack.c
        cdatapack.h
        convert.h)

# find_library can't seem to come to terms with searching for lz4 on my
# laptop.  it insists on returning /usr/lib/libl.a, which makes no sense to me.
#
# after wasting a bunch of time debugging this, I gave up because nobody else
# uses CLion to build this. :)
SET(LZ4 "/opt/local/lib/liblz4.a")
#find_library(LZ4 NAMES lz4
#             PATHS /opt/local/lib)
#MESSAGE( STATUS "WTF LZ4: " ${LZ4} )
target_include_directories(cdatapack PUBLIC /opt/local/include)
target_link_libraries(cdatapack PUBLIC ${LZ4})

add_executable(cdatapack_dump cdatapack_dump.c)
target_link_libraries(cdatapack_dump cdatapack)

add_executable(cdatapack_get cdatapack_get.c)
target_link_libraries(cdatapack_get cdatapack)

find_library(OPENSSL
        NAMES crypto
        PATHS /opt/local/lib)
target_include_directories(cdatapack_get PUBLIC /opt/local/include)
target_link_libraries(cdatapack_get ${OPENSSL})

add_executable(null_test null_test.c)
