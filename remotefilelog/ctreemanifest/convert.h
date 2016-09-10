// convert.h - conversion utility methods
//
// Copyright 2016 Facebook, Inc.
//
// no-check-code
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#ifndef CTREEMANIFEST_CONVERT_H
#define CTREEMANIFEST_CONVERT_H

#include <stdint.h>

#include <string>

#define HEX_NODE_SIZE 40
#define BIN_NODE_SIZE 20

static int8_t hextable[256] = {
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
     0,  1,  2,  3,  4,  5,  6,  7,  8,  9, -1, -1, -1, -1, -1, -1, /* 0-9 */
    -1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* A-F */
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, 10, 11, 12, 13, 14, 15, -1, -1, -1, -1, -1, -1, -1, -1, -1, /* a-f */
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1
};

static char chartable[16] = {
    '0', '1', '2', '3', '4', '5', '6', '7',
    '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'
};

/**
 * Converts a given 40-byte hex string into a 20-byte node.
 */
inline void appendbinfromhex(const char *node, std::string &output) {
  for (int i = 0; i < HEX_NODE_SIZE;) {
    int8_t hi = hextable[(unsigned char)node[i++]];
    int8_t lo = hextable[(unsigned char)node[i++]];
    output.push_back((hi << 4) | lo);
  }
}

/**
 * Converts a given 40-byte hex string into a 20-byte node.
 */
inline std::string binfromhex(const char *node) {
  std::string result;

  result.reserve(BIN_NODE_SIZE);
  appendbinfromhex(node, result);
  return result;
}

/**
 * Converts a given 20-byte node into a 40-byte hex string.
 */
inline void hexfrombin(const char *binnode, std::string &output) {
  for (size_t ix = 0; ix < BIN_NODE_SIZE; ix++) {
    unsigned char ch = (unsigned char) binnode[ix];
    char hi = chartable[ch >> 4];
    char lo = chartable[ch & 0xf];

    output.push_back(hi);
    output.push_back(lo);
  }
}

#endif //CTREEMANIFEST_CONVERT_H
