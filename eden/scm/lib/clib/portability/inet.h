// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef FBHGEXT_CLIB_PORTABILITY_INET_H
#define FBHGEXT_CLIB_PORTABILITY_INET_H

#if defined(_MSC_VER)
#include <winsock2.h>
#pragma comment(lib, "Ws2_32.lib")
/* See https://fburl.com/7hd350j8 for more details about Ws2_32.lib */
#else
#include <arpa/inet.h>
#endif

#endif /* FBHGEXT_CLIB_PORTABILITY_INET_H */
