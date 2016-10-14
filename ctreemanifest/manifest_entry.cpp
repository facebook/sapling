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
  this->ownedmemory = NULL;
}

void ManifestEntry::initialize(
    const char *filename, const size_t filenamelen,
    const char *node,
    const char *flag) {
  if (flag != NULL && *flag == MANIFEST_DIRECTORY_FLAG) {
    this->resolved = ManifestPtr(new Manifest());
  }
  this->ownedmemory = new char[
  filenamelen +
  1 +              // null character
  HEX_NODE_SIZE +  // node hash
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
    memcpy(this->node, node, HEX_NODE_SIZE);
  }
  if (flag == NULL) {
    *(this->filename + filenamelen + 1 + HEX_NODE_SIZE) = '\n';
    this->flag = NULL;
  } else {
    this->flag = this->filename + filenamelen + 1 + HEX_NODE_SIZE;
    *this->flag = *flag;
  }
}

char *ManifestEntry::initialize(char *entrystart) {
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
  char *nextpointer;
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

void ManifestEntry::initialize(ManifestEntry *other) {
  if (other->ownedmemory) {
    this->initialize(other->filename, other->filenamelen,
        other->node, other->flag);
    if (other->resolved.isnull()) {
      this->resolved = ManifestPtr();
    }
  } else {
    // Else it points at a piece of memory owned by something else
    this->initialize(other->filename);
  }

  if (!other->resolved.isnull()) {
    this->resolved = other->resolved->copy();
  }
}

ManifestEntry::~ManifestEntry() {
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

ManifestPtr ManifestEntry::get_manifest(
    ManifestFetcher fetcher, const char *path, size_t pathlen) {
  if (this->resolved.isnull()) {
    std::string binnode = binfromhex(node);
    this->resolved = fetcher.get(path, pathlen, binnode);
  }

  return this->resolved;
}

void ManifestEntry::update(const char *node, const char *flag) {
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
  if (this->node == NULL) {
    this->node = this->filename + this->filenamelen + 1;
  }
  memcpy(this->node, node, HEX_NODE_SIZE);

  if (flag == NULL) {
    *(this->filename + this->filenamelen + 1 + HEX_NODE_SIZE) = '\n';
    this->flag = NULL;
  } else {
    this->flag = this->filename + filenamelen + 1 + HEX_NODE_SIZE;
    *this->flag = *flag;
  }
}

static size_t mercurialOrderFilenameLength(const ManifestEntry &entry) {
  return entry.filenamelen +
         ((entry.flag != NULL && *entry.flag == MANIFEST_DIRECTORY_FLAG) ?
          1 : 0);
}

static char mercurialOrderFilenameCharAt(
    const ManifestEntry &entry, size_t offset) {
  if (offset < entry.filenamelen) {
    return entry.filename[offset];
  } else if (offset == entry.filenamelen &&
      (entry.flag != NULL && *entry.flag == MANIFEST_DIRECTORY_FLAG)) {
    return '/';
  }

  throw std::out_of_range("Illegal index for manifest entry");
}

bool ManifestEntry::compareMercurialOrder(
    ManifestEntry * const &left,
    ManifestEntry * const &right) {
  size_t leftlen = mercurialOrderFilenameLength(*left);
  size_t rightlen = mercurialOrderFilenameLength(*right);
  size_t minlen = (leftlen < rightlen) ? leftlen : rightlen;

  for (size_t ix = 0; ix < minlen; ix ++) {
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

int ManifestEntry::compareName(ManifestEntry *left, ManifestEntry *right) {
  assert(left || right);

  // If left is empty, then it is greater than right. This makes this function
  // useful for iterating right after left has already finished.
  if (!left) {
    return 1;
  }
  else if (!right) {
    return -1;
  }

  size_t minlen = left->filenamelen < right->filenamelen ?
                  left->filenamelen : right->filenamelen;
  int cmp = strncmp(left->filename, right->filename, minlen);
  if (cmp == 0 && left->filenamelen == right->filenamelen) {
    return 0;
  } else if (cmp > 0 ||
             (cmp == 0 && left->filenamelen > right->filenamelen)) {
    return 1;
  } else {
    return -1;
  }
}
