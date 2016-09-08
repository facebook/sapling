// manifest_entry.cpp - c++ implementation of a single manifest entry
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "manifest_entry.h"

ManifestEntry::ManifestEntry() {
  this->filename = NULL;
  this->filenamelen = 0;
  this->node = NULL;
  this->flag = NULL;
  this->resolved = NULL;
  this->ownedmemory = NULL;
}

void ManifestEntry::initialize(
    const char *filename, const size_t filenamelen,
    const char *node,
    char flag) {
  if (flag == MANIFEST_DIRECTORY_FLAG) {
    this->resolved = new Manifest();
  }
  this->ownedmemory = new char[
  filenamelen +
  1 +              // null character
  40 +             // node hash
  1 +              // flag
  1                // NL
  ];

  // set up the pointers.
  this->filename = this->ownedmemory;
  if (node == NULL) {
    this->node = NULL;
  } else {
    this->node = this->filename + filenamelen + 1;
  }

  // set up the null character and NL.
  this->filename[filenamelen] = '\0';
  *(this->filename + filenamelen + 1 + HEX_NODE_SIZE + 1) = '\n';

  // set up filenamelen
  this->filenamelen = filenamelen;

  // write the memory.
  memcpy(this->filename, filename, filenamelen);
  if (node != NULL) {
    memcpy(this->node, node, 40);
  }
  if (flag == '\0') {
    *(this->filename + filenamelen + 1 + HEX_NODE_SIZE) = '\n';
    this->flag = NULL;
  } else {
    this->flag = this->filename + filenamelen + 1 + HEX_NODE_SIZE;
    *this->flag = flag;
  }
}

/**
 * Given the start of a file/dir entry in a manifest, returns a
 * ManifestEntry structure with the parsed data.
 */
ManifestEntry::ManifestEntry(char *&entrystart) {
  // Each entry is of the format:
  //
  //   <filename>\0<40-byte hash><optional 1 byte flag>\n
  //
  // Where flags can be 't' to represent a sub directory
  this->filename = entrystart;
  char *nulldelimiter = strchr(entrystart, '\0');
  this->filenamelen = nulldelimiter - entrystart;

  this->node = nulldelimiter + 1;

  this->flag = nulldelimiter + 41;
  if (*this->flag != '\n') {
    entrystart = this->flag + 2;
  } else {
    // No flag
    entrystart = this->flag + 1;
    this->flag = NULL;
  }
  this->resolved = NULL;
  this->ownedmemory = NULL;
}

ManifestEntry::~ManifestEntry() {
  if (this->resolved != NULL) {
    delete this->resolved;
  }
  if (this->ownedmemory != NULL) {
    delete [] this->ownedmemory;
  }
}

bool ManifestEntry::isdirectory() const {
  return this->flag && *this->flag == MANIFEST_DIRECTORY_FLAG;
}

void ManifestEntry::appendtopath(std::string &path) {
  path.append(this->filename, this->filenamelen);
  if (this->isdirectory()) {
    path.append(1, '/');
  }
}

Manifest *ManifestEntry::get_manifest(
    ManifestFetcher fetcher, const char *path, size_t pathlen) {
  if (this->resolved == NULL) {
    std::string binnode = binfromhex(node);
    this->resolved = fetcher.get(path, pathlen, binnode);
  }

  return this->resolved;
}
