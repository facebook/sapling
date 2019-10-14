/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once
#include <string>
#include "eden/fs/utils/PathFuncs.h"
#include "eden/fs/win/utils/Handle.h"

namespace facebook {
namespace eden {

using RegistryPath = const wchar_t*;
using RegistryName = const wchar_t*;
using ValueName = std::wstring;

struct RegHandleTraits {
  using Type = HKEY;

  static Type invalidHandleValue() noexcept {
    return nullptr;
  }
  static void close(Type handle) noexcept {
    RegCloseKey(handle);
  }
};

using RegHandle = HandleBase<RegHandleTraits>;

/**
 * RegistryKey represents an open instance of registry key
 **/

class RegistryKey {
 public:
  /*
   * No copy construction or assignment allowed.
   */
  RegistryKey(RegistryKey const&) = delete;
  RegistryKey& operator=(RegistryKey const&) = delete;

  /*
   * Move operations are permitted
   */

  RegistryKey(RegistryKey&& other) noexcept
      : handle_(std::move(other.handle_)) {}

  RegistryKey& operator=(RegistryKey&& other) noexcept {
    if (this != &other) {
      handle_ = std::move(other.handle_);
    }
    return *this;
  }

  static RegistryKey create(
      HKEY parent,
      RegistryPath keyname,
      REGSAM access = KEY_ALL_ACCESS,
      DWORD* disposition = nullptr,
      DWORD options = REG_OPTION_NON_VOLATILE,
      SECURITY_ATTRIBUTES* securityAttributes = nullptr);

  static RegistryKey createCurrentUser(
      RegistryPath keyname,
      REGSAM access = KEY_ALL_ACCESS,
      DWORD* disposition = nullptr,
      DWORD options = REG_OPTION_NON_VOLATILE,
      SECURITY_ATTRIBUTES* securityAttributes = nullptr) {
    return RegistryKey::create(
        HKEY_CURRENT_USER,
        keyname,
        access,
        disposition,
        options,
        securityAttributes);
  }

  static RegistryKey createUsers(
      RegistryPath keyname,
      REGSAM access = KEY_ALL_ACCESS,
      DWORD* disposition = nullptr,
      DWORD options = REG_OPTION_NON_VOLATILE,
      SECURITY_ATTRIBUTES* securityAttributes = nullptr) {
    RegistryKey::create(
        HKEY_USERS, keyname, access, disposition, options, securityAttributes);
  }

  static RegistryKey createLocalMachine(
      RegistryPath keyname,
      REGSAM access = KEY_ALL_ACCESS,
      DWORD* disposition = nullptr,
      DWORD options = REG_OPTION_NON_VOLATILE,
      SECURITY_ATTRIBUTES* securityAttributes = nullptr) {
    RegistryKey::create(
        HKEY_LOCAL_MACHINE,
        keyname,
        access,
        disposition,
        options,
        securityAttributes);
  }

  RegistryKey create(
      const RegistryName keyname,
      REGSAM access = KEY_ALL_ACCESS,
      DWORD* disposition = nullptr,
      DWORD options = REG_OPTION_NON_VOLATILE,
      SECURITY_ATTRIBUTES* securityAttributes = nullptr) {
    return create(
        handle_.get(),
        keyname,
        access,
        disposition,
        options,
        securityAttributes);
  }

  static RegistryKey open(
      HKEY parent,
      RegistryPath keyName,
      const REGSAM desiredAccess = KEY_ALL_ACCESS);

  static RegistryKey openCurrentUser(
      RegistryPath keyName,
      const REGSAM desiredAccess = KEY_ALL_ACCESS) {
    return RegistryKey::open(HKEY_CURRENT_USER, keyName, desiredAccess);
  }

  static RegistryKey openLocalMachine(
      RegistryPath keyName,
      const REGSAM desiredAccess = KEY_ALL_ACCESS) {
    return RegistryKey::open(HKEY_LOCAL_MACHINE, keyName, desiredAccess);
  }

  static RegistryKey openUsers(
      RegistryPath keyName,
      const REGSAM desiredAccess = KEY_ALL_ACCESS) {
    return RegistryKey::open(HKEY_USERS, keyName, desiredAccess);
  }

  RegistryKey openSubKey(
      RegistryPath keyName,
      const REGSAM desiredAccess = KEY_ALL_ACCESS) const {
    return open(handle_.get(), keyName, desiredAccess);
  }

  std::vector<std::wstring> enumerateKeys() const;

  void deleteKey(RegistryPath subKey = nullptr);

  static void
  renameKey(HKEY root, RegistryPath newName, RegistryPath keyName = nullptr);

  void renameKey(RegistryPath newName, RegistryPath keyName = nullptr);

  /*
   * Function for getting and setting values
   */
  DWORD
  getDWord(const ValueName& value, RegistryPath subKey = nullptr) const;

  std::wstring getString(const ValueName& value, RegistryPath subKey = nullptr)
      const;

  DWORD getBinary(
      const ValueName& value,
      void* buffer,
      DWORD size,
      RegistryPath subKey = nullptr) const;

  void setDWord(const ValueName& value, const DWORD data) const;
  void setString(const ValueName& value, const std::wstring& data) const;
  void setBinary(const ValueName& value, const void* data, size_t size) const;

  /**
   * enumerateValues will fetch all the values under the given key and their
   * types.
   **/
  std::vector<std::pair<std::wstring, DWORD>> enumerateValues();

  void deleteValue(const ValueName& value);

  ~RegistryKey() {}
  RegistryKey() {}

 private:
  RegistryKey(RegHandle&& handle) : handle_(std::move(handle)) {}

  RegHandle handle_;
};

} // namespace eden
} // namespace facebook
