// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// manifest_entry.cpp - c++ implementation of a single manifest entry
// no-check-code

#include "edenscm/hgext/extlib/ctreemanifest/manifest_entry.h"

#include <cassert>

#include "edenscm/hgext/extlib/ctreemanifest/manifest.h"

ManifestEntry::ManifestEntry()
    : node(NULL),
      filename(NULL),
      filenamelen(0),
      flag(NULL),
      ownedmemory(NULL) {}

void ManifestEntry::initialize(
    const char* filename,
    const size_t filenamelen,
    const char* node,
    const char* flag) {
  if (flag != NULL && *flag == MANIFEST_DIRECTORY_FLAG) {
    this->resolved = ManifestPtr(new Manifest());
  }

  assert(!this->ownedmemory);

  this->ownedmemory = new char
      [filenamelen + 1 + // null character
       HEX_NODE_SIZE + // node hash
       1 + // flag
       1 // NL
  ];

  // We'll use this as a cursor into the memory blob we just allocated
  char* buf = this->ownedmemory;

  char* filenamecopy = buf;
  if (filenamelen > 0) {
    memcpy(filenamecopy, filename, filenamelen);
  }
  filenamecopy[filenamelen] = '\0';
  buf += filenamelen + 1;

  char* nodecopy = NULL;
  if (node) {
    nodecopy = buf;
    memcpy(nodecopy, node, HEX_NODE_SIZE);
  }
  // Note that the region where the node would have gone has undefined
  // contents in the case that node is NULL, but it is there
  // and reserved ready for use by updatehexnode().
  buf += HEX_NODE_SIZE;

  char* flagcopy = NULL;
  if (flag) {
    flagcopy = buf;
    *flagcopy = *flag;
    buf += 1;
  }

  *buf = '\n';

  this->filename = filenamecopy;
  this->filenamelen = filenamelen;
  this->node = nodecopy;
  this->flag = flagcopy;
}

const char* ManifestEntry::initialize(const char* entrystart) {
  // Each entry is of the format:
  //
  //   <filename>\0<40-byte hash><optional 1 byte flag>\n
  //
  // Where flags can be 't' to represent a sub directory
  this->filename = entrystart;
  const char* nulldelimiter = strchr(entrystart, '\0');
  this->filenamelen = nulldelimiter - entrystart;

  this->node = nulldelimiter + 1;

  this->flag = nulldelimiter + 41;
  const char* nextpointer;
  if (*this->flag != '\n') {
    nextpointer = this->flag + 2;
  } else {
    // No flag
    nextpointer = this->flag + 1;
    this->flag = NULL;
  }
  this->resolved = ManifestPtr();
  this->ownedmemory = NULL;

  return nextpointer;
}

void ManifestEntry::initialize(ManifestEntry* other) {
  if (other->ownedmemory) {
    this->initialize(
        other->filename, other->filenamelen, other->node, other->flag);
    if (other->resolved.isnull()) {
      this->resolved = ManifestPtr();
    }
  } else {
    // Else it points at a piece of memory owned by something else
    this->initialize(other->filename);
  }

  if (!other->resolved.isnull()) {
    if (other->resolved->isMutable()) {
      this->resolved = other->resolved->copy();
    } else {
      this->resolved = other->resolved;
    }
  }
}

ManifestEntry::~ManifestEntry() {
  if (this->ownedmemory != NULL) {
    delete[] this->ownedmemory;
  }
}

bool ManifestEntry::isdirectory() const {
  return this->flag && *this->flag == MANIFEST_DIRECTORY_FLAG;
}

bool ManifestEntry::hasNode() const {
  return this->node;
}

const char* ManifestEntry::get_node() {
  if (!this->node && this->flag && *this->flag == MANIFEST_DIRECTORY_FLAG &&
      !this->resolved.isnull() && !this->resolved->isMutable()) {
    this->updatebinnode(this->resolved->node(), MANIFEST_DIRECTORY_FLAGPTR);
  }
  return this->node;
}

void ManifestEntry::reset_node() {
  this->node = NULL;
}

void ManifestEntry::appendtopath(std::string& path) {
  path.append(this->filename, this->filenamelen);
  if (this->isdirectory()) {
    path.append(1, '/');
  }
}

ManifestPtr ManifestEntry::get_manifest(
    const ManifestFetcher& fetcher,
    const char* path,
    size_t pathlen) {
  if (this->resolved.isnull()) {
    std::string binnode = binfromhex(node);
    // Chop off the trailing slash
    if (pathlen > 0) {
      if (path[pathlen - 1] == '/') {
        --pathlen;
      }
    }
    this->resolved = fetcher.get(path, pathlen, binnode);
  }

  return this->resolved;
}

void ManifestEntry::updatebinnode(const char* node, const char* flag) {
  std::string hexnode;
  hexfrombin(node, hexnode);
  this->updatehexnode(hexnode.c_str(), flag);
}

void ManifestEntry::updatehexnode(const char* node, const char* flag) {
  // we cannot flip between file and directory.
  bool wasdir = this->flag != NULL && *this->flag == MANIFEST_DIRECTORY_FLAG;
  bool willbedir = flag != NULL && *flag == MANIFEST_DIRECTORY_FLAG;

  if (wasdir != willbedir) {
    throw std::logic_error("changing to/from directory is not permitted");
  }

  // if we didn't previously own the memory, we should now.
  if (this->ownedmemory == NULL) {
    ManifestPtr oldresolved = this->resolved;
    this->initialize(this->filename, this->filenamelen, node, flag);
    this->resolved = oldresolved;
    return;
  }

  // initialize node if it's not already done.
  if (!this->hasNode()) {
    this->node = this->filename + this->filenamelen + 1;
  }

  // The const_cast is safe because we checked this->ownedmemory above.
  char* owned_node = const_cast<char*>(this->node);
  memcpy(owned_node, node, HEX_NODE_SIZE);

  if (flag == NULL) {
    owned_node[HEX_NODE_SIZE] = '\n';
    this->flag = NULL;
  } else {
    owned_node[HEX_NODE_SIZE] = *flag;
    owned_node[HEX_NODE_SIZE + 1] = '\n';
    this->flag = owned_node + HEX_NODE_SIZE;
  }
}

static size_t mercurialOrderFilenameLength(const ManifestEntry& entry) {
  return entry.filenamelen +
      ((entry.flag != NULL && *entry.flag == MANIFEST_DIRECTORY_FLAG) ? 1 : 0);
}

static char mercurialOrderFilenameCharAt(
    const ManifestEntry& entry,
    size_t offset) {
  if (offset < entry.filenamelen) {
    return entry.filename[offset];
  } else if (
      offset == entry.filenamelen &&
      (entry.flag != NULL && *entry.flag == MANIFEST_DIRECTORY_FLAG)) {
    return '/';
  }

  throw std::out_of_range("Illegal index for manifest entry");
}

bool ManifestEntry::compareMercurialOrder(
    ManifestEntry* const& left,
    ManifestEntry* const& right) {
  size_t leftlen = mercurialOrderFilenameLength(*left);
  size_t rightlen = mercurialOrderFilenameLength(*right);
  size_t minlen = (leftlen < rightlen) ? leftlen : rightlen;

  for (size_t ix = 0; ix < minlen; ix++) {
    unsigned char leftchar = mercurialOrderFilenameCharAt(*left, ix);
    unsigned char rightchar = mercurialOrderFilenameCharAt(*right, ix);

    if (leftchar < rightchar) {
      return true;
    } else if (leftchar > rightchar) {
      return false;
    }
  }

  // same up to minlen.
  if (leftlen < rightlen) {
    return true;
  }

  return false;
}

int ManifestEntry::compareName(ManifestEntry* left, ManifestEntry* right) {
  assert(left || right);

  // If left is empty, then it is greater than right. This makes this function
  // useful for iterating right after left has already finished.
  if (!left) {
    return 1;
  } else if (!right) {
    return -1;
  }

  size_t minlen = left->filenamelen < right->filenamelen ? left->filenamelen
                                                         : right->filenamelen;
  int cmp = strncmp(left->filename, right->filename, minlen);
  if (cmp == 0 && left->filenamelen == right->filenamelen) {
    return 0;
  } else if (cmp > 0 || (cmp == 0 && left->filenamelen > right->filenamelen)) {
    return 1;
  } else {
    return -1;
  }
}
