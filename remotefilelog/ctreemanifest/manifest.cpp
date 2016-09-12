// manifest.cpp - c++ implementation of a single manifest
//
// Copyright 2016 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code

#include "manifest.h"

Manifest::Manifest(PythonObj &rawobj) :
    _rawobj(rawobj) {
  char *parseptr, *endptr;
  Py_ssize_t buf_sz;
  PyString_AsStringAndSize(_rawobj, &parseptr, &buf_sz);
  endptr = parseptr + buf_sz;

  while (parseptr < endptr) {
    ManifestEntry entry = ManifestEntry(parseptr);
    entries.push_back(entry);
  }
}

Manifest *Manifest::copy() {
  Manifest *copied = new Manifest(this->_rawobj);
  std::list<ManifestEntry>::iterator copyIter = copied->entries.begin();

  for (std::list<ManifestEntry>::iterator thisIter = this->entries.begin();
       thisIter != this->entries.end();
       thisIter ++) {
    copied->addChild(copyIter, &(*thisIter));
  }

  return copied;
}

ManifestIterator Manifest::getIterator() {
  return ManifestIterator(this->entries.begin(), this->entries.end());
}

/**
 * Returns an iterator correctly positioned for a child of a given
 * filename.  If a child with the same name already exists, *exacthit will
 * be set to true.  Otherwise, it will be set to false.
 */
std::list<ManifestEntry>::iterator Manifest::findChild(
    const char *filename, const size_t filenamelen,
    bool *exacthit) {
  for (std::list<ManifestEntry>::iterator iter = this->entries.begin();
       iter != this->entries.end();
       iter ++) {
    size_t minlen = filenamelen < iter->filenamelen ?
                    filenamelen : iter->filenamelen;

    // continue until we are lexicographically <= than the current location.
    int cmp = strncmp(filename, iter->filename, minlen);
    if (cmp == 0 && filenamelen == iter->filenamelen) {
      *exacthit = true;
      return iter;
    } else if (cmp > 0 ||
        (cmp == 0 && filenamelen > iter->filenamelen)) {
      continue;
    } else {
      *exacthit = false;
      return iter;
    }
  }

  *exacthit = false;
  return this->entries.end();
}

ManifestEntry *Manifest::addChild(std::list<ManifestEntry>::iterator iterator,
    const char *filename, const size_t filenamelen, const char *node,
    const char flag) {
  ManifestEntry entry;
  this->entries.insert(iterator, entry);

  // move back to the element we just added.
  --iterator;

  // return a reference to the element we added, not the one on the stack.
  ManifestEntry *result = &(*iterator);

  result->initialize(filename, filenamelen, node, flag);

  return result;
}

ManifestEntry *Manifest::addChild(std::list<ManifestEntry>::iterator iterator,
        ManifestEntry *otherChild) {
  ManifestEntry entry;
  this->entries.insert(iterator, entry);

  // move back to the element we just added.
  --iterator;

  // return a reference to the element we added, not the one on the stack.
  ManifestEntry *result = &(*iterator);

  result->initialize(otherChild);

  return result;
}

ManifestIterator::ManifestIterator(
    std::list<ManifestEntry>::iterator iterator,
    std::list<ManifestEntry>::const_iterator end) :
    iterator(iterator), end(end) {
}

ManifestEntry *ManifestIterator::next() {
  if (this->isfinished()) {
    return NULL;
  }

  ManifestEntry *result = &(*this->iterator);
  this->iterator ++;

  return result;
}

ManifestEntry *ManifestIterator::currentvalue() const {
  if (this->isfinished()) {
    throw std::logic_error("iterator has no current value");
  }

  return &(*iterator);
}

bool ManifestIterator::isfinished() const {
  return iterator == end;
}

void Manifest::serialize(std::string &result) {
  result.erase();
  result.reserve(16 * 1024 * 1024);
  ManifestIterator iterator = this->getIterator();
  ManifestEntry *entry;
  while ((entry = iterator.next()) != NULL) {
    result.append(entry->filename, entry->filenamelen);
    result.append("\0");
    result.append(entry->node, HEX_NODE_SIZE);
    if (entry->flag) {
      result.append(1, *entry->flag);
    }
    result.append("\n");
  }
}
