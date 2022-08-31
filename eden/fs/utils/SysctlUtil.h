/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <string>

#ifdef __APPLE__
// Fetches the value of a sysctl by name.
// The result is assumed to be a string.
std::string getSysCtlByName(const char* name, size_t size);
#endif
