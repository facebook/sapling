// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#include "edenscm/hgext/extlib/ctreemanifest/manifest_ptr.h"

#include <cstddef>
#include <stdexcept>

#include "edenscm/hgext/extlib/ctreemanifest/manifest.h"

ManifestPtr::ManifestPtr() : manifest(NULL) {}

ManifestPtr::ManifestPtr(Manifest* manifest) : manifest(manifest) {
  if (!manifest) {
    throw std::logic_error("passed NULL manifest pointer");
  }
  this->manifest->incref();
}

ManifestPtr::ManifestPtr(const ManifestPtr& other) : manifest(other.manifest) {
  if (this->manifest) {
    this->manifest->incref();
  }
}

ManifestPtr::~ManifestPtr() {
  if (this->manifest && this->manifest->decref() == 0) {
    delete (this->manifest);
  }
}

ManifestPtr& ManifestPtr::operator=(const ManifestPtr& other) {
  if (this->manifest) {
    this->manifest->decref();
  }
  this->manifest = other.manifest;
  if (this->manifest) {
    this->manifest->incref();
  }
  return *this;
}

ManifestPtr::operator Manifest*() const {
  return this->manifest;
}

Manifest* ManifestPtr::operator->() const {
  return this->manifest;
}

bool ManifestPtr::isnull() const {
  return this->manifest == NULL;
}
