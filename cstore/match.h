// match.h - c++ declarations for a data store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code
//
#ifndef MATCH_H
#define MATCH_H

class Matcher {
  public:
    virtual ~Matcher() {}
    virtual bool matches(const std::string &path) = 0;
    virtual bool matches(const char *path, const size_t pathlen) = 0;
    virtual bool visitdir(const std::string &path) = 0;
};

class AlwaysMatcher : public Matcher {
  public:
    AlwaysMatcher() {}
    virtual ~AlwaysMatcher() {}
    virtual bool matches(const std::string &path) { return true; }
    virtual bool matches(const char *path, const size_t pathlen) { return true; }
    virtual bool visitdir(const std::string &path) { return true; }
};

#endif // MATCH_H
