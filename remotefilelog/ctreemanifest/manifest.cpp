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

ManifestIterator Manifest::getIterator() const {
  return ManifestIterator(this->entries.begin(), this->entries.end());
}

ManifestIterator::ManifestIterator(
    std::list<ManifestEntry>::const_iterator iterator,
    std::list<ManifestEntry>::const_iterator end) :
    iterator(iterator), end(end) {
}

bool ManifestIterator::next(ManifestEntry *entry) {
  if (this->isfinished()) {
    return false;
  }

  *entry = *this->iterator;
  this->iterator++;

  return true;
}

ManifestEntry ManifestIterator::currentvalue() const {
  if (this->isfinished()) {
    throw std::logic_error("iterator has no current value");
  }

  return *iterator;
}

bool ManifestIterator::isfinished() const {
  return iterator == end;
}
