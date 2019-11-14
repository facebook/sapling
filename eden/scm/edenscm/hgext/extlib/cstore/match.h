/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// match.h - c++ declarations for a data store
// no-check-code

#ifndef FBHGEXT_CSTORE_MATCH_H
#define FBHGEXT_CSTORE_MATCH_H

class Matcher {
 public:
  virtual ~Matcher() {}
  virtual bool matches(const std::string& path) = 0;
  virtual bool matches(const char* path, const size_t pathlen) = 0;
  virtual bool visitdir(const std::string& path) = 0;
};

class AlwaysMatcher : public Matcher {
 public:
  AlwaysMatcher() {}
  ~AlwaysMatcher() override {}
  bool matches(const std::string& /*path*/) override {
    return true;
  }
  bool matches(const char* /*path*/, const size_t /*pathlen*/) override {
    return true;
  }
  bool visitdir(const std::string& /*path*/) override {
    return true;
  }
};

#endif // FBHGEXT_CSTORE_MATCH_H
