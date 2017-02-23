// store.h - c++ declarations for a data store
//
// Copyright 2017 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
//
// no-check-code
//
#ifndef STORE_H
#define STORE_H

#include "key.h"

class ConstantString {
  friend class ConstantStringRef;
  private:
    char *_content;
    size_t _size;
    size_t _refCount;

    ConstantString(char *content, size_t size) :
      _content(content),
      _size(size),
      _refCount(1) {}
  public:
    ~ConstantString() {
      delete _content;
    }

    char *content() {
      return _content;
    }

    size_t size() {
      return _size;
    }

    void incref() {
      _refCount++;
    }

    size_t decref() {
      if (_refCount > 0) {
        _refCount--;
      }
      return _refCount;
    }
};

class ConstantStringRef {
  private:
    ConstantString *_str;
  public:
    explicit ConstantStringRef() :
      _str(NULL) {
    }

    ConstantStringRef(char *str, size_t size) :
      _str(new ConstantString(str, size)) {
    }

    ConstantStringRef(const ConstantStringRef &other) {
      if (other._str) {
        other._str->incref();
      }
      _str = other._str;
    }

    ~ConstantStringRef() {
      if (_str && _str->decref() == 0) {
        delete _str;
      }
    }

    ConstantStringRef& operator=(const ConstantStringRef &other) {
      if (_str && _str->decref() == 0) {
        delete _str;
      }
      _str = other._str;
      if (_str) {
        _str->incref();
      }
      return *this;
    }

    char *content() {
      if (_str) {
        return _str->content();
      }

      return NULL;
    }

    size_t size() {
      if (_str) {
        return _str->size();
      }

      return 0;
    }
};

class Store {
  public:
    virtual ~Store() {}
    virtual ConstantStringRef get(const Key &key) = 0;
};

#endif //STORE_H
