// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

// no-check-code

#ifndef FBHGEXT_CTREEMANIFEST_MANIFEST_PTR_H
#define FBHGEXT_CTREEMANIFEST_MANIFEST_PTR_H

class Manifest;

class ManifestPtr {
 private:
  Manifest* manifest;

 public:
  ManifestPtr();

  ManifestPtr(Manifest* manifest);

  ManifestPtr(const ManifestPtr& other);

  ~ManifestPtr();

  ManifestPtr& operator=(const ManifestPtr& other);

  operator Manifest*() const;

  Manifest* operator->() const;

  bool isnull() const;
};

#endif /* FBHGEXT_CTREEMANIFEST_MANIFEST_PTR_H */
