/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include "folly/portability/Windows.h"

#include "eden/fs/win/utils/Handle.h"
#include "eden/fs/win/utils/RegUtils.h"
#include "eden/fs/win/utils/StringConv.h"
#include "eden/fs/win/utils/WinError.h"

namespace facebook {
namespace eden {

RegistryKey RegistryKey::create(
    HKEY parent,
    RegistryPath keyname,
    REGSAM access,
    DWORD* disposition,
    DWORD options,
    SECURITY_ATTRIBUTES* securityAttributes) {
  RegHandle handle;
  LSTATUS status = RegCreateKeyEx(
      parent,
      keyname,
      0,
      nullptr,
      options,
      access,
      securityAttributes,
      handle.set(),
      disposition);
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to create the key : {}", wideToMultibyteString(keyname)));
  }

  return RegistryKey{std::move(handle)};
}

RegistryKey RegistryKey::open(
    const HKEY parent,
    RegistryPath keyname,
    const REGSAM desiredAccess) {
  RegHandle handle;
  LSTATUS status = RegOpenKeyExW(
      parent, keyname, RRF_RT_REG_NONE, desiredAccess, handle.set());

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to open the key : {}", wideToMultibyteString(keyname)));
  }

  return RegistryKey{std::move(handle)};
}

std::vector<std::wstring> RegistryKey::enumerateKeys() const {
  DWORD numKeys{};
  DWORD maxKeyLength{};
  LSTATUS status = RegQueryInfoKey(
      handle_.get(),
      nullptr,
      nullptr,
      nullptr,
      &numKeys,
      &maxKeyLength,
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      nullptr);

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(status, "Failed to query the reg key info");
  }

  // Add some buffer (space for NULL character)
  maxKeyLength += 32;
  auto nameBuffer = std::make_unique<wchar_t[]>(maxKeyLength);
  std::vector<std::wstring> subkeyNames;

  for (DWORD index = 0; index < numKeys;) {
    DWORD subKeyNameLen = maxKeyLength;
    status = RegEnumKeyEx(
        handle_.get(),
        index,
        nameBuffer.get(),
        &subKeyNameLen,
        nullptr,
        nullptr,
        nullptr,
        nullptr);
    if (status == ERROR_MORE_DATA) {
      // We could only get here if a key was inserted between our call to
      // RegQueryInfoKey and RegEnumKeyEx with bigger length. Reallocate out
      // buffer and continue.

      maxKeyLength = subKeyNameLen + 32;
      nameBuffer = std::make_unique<wchar_t[]>(maxKeyLength);
      continue;
    } else if (status == ERROR_NO_MORE_ITEMS) {
      // We could only get here if a key was deleted.
      break;
    }
    if (status != ERROR_SUCCESS) {
      throw makeWin32ErrorExplicit(status, "Enumeration failed");
    }

    subkeyNames.emplace_back(std::wstring{nameBuffer.get(), subKeyNameLen});
    index++;
  }

  return subkeyNames;
}

void RegistryKey::deleteKey(RegistryPath subKey) {
  LSTATUS status = RegDeleteTreeW(handle_.get(), subKey);
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(status, "Failed to delete the key");
  }
  const wchar_t* str = L"";
  RegDeleteKey(handle_.get(), str);
}

void RegistryKey::renameKey(
    HKEY root,
    RegistryPath newName,
    RegistryPath keyName) {
  LSTATUS status = RegRenameKey(root, keyName, newName);

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to rename the key: {} -> {}",
            wideToMultibyteString(keyName),
            wideToMultibyteString(newName)));
  }
}

void RegistryKey::renameKey(RegistryPath newName, RegistryPath keyName) {
  renameKey(handle_.get(), newName, keyName);
}

DWORD
RegistryKey::getDWord(const ValueName& value, RegistryPath subKey) const {
  DWORD data{};
  DWORD size = sizeof(data);

  LSTATUS status = RegGetValueW(
      handle_.get(),
      subKey,
      value.c_str(),
      RRF_RT_REG_DWORD,
      nullptr,
      &data,
      &size);
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to get 32bit value from Registry : {}:{}",
            wideToMultibyteString(subKey),
            wideToMultibyteString(value)));
  }
  return data;
}

std::wstring RegistryKey::getString(const ValueName& value, RegistryPath subKey)
    const {
  // This string could have any size. Let's not make assumptions about the size
  // and get the length upfront.
  DWORD size{};
  LSTATUS status = RegGetValue(
      handle_.get(),
      subKey,
      value.c_str(),
      RRF_RT_REG_SZ,
      nullptr,
      nullptr,
      &size);
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to get string value from Registry: {}:{}",
            wideToMultibyteString(subKey),
            wideToMultibyteString(value)));
  }

  std::wstring data;
  data.resize(size / sizeof(wchar_t));
  status = RegGetValue(
      handle_.get(),
      subKey,
      value.c_str(),
      RRF_RT_REG_SZ,
      nullptr,
      data.data(),
      &size);

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to get string value from Registry {}:{} size: {}",
            wideToMultibyteString(subKey),
            wideToMultibyteString(value),
            size));
  }

  DWORD length = size / sizeof(wchar_t);
  length--; // size includes a null char at the end.
  data.resize(length);
  return data;
}

DWORD RegistryKey::getBinary(
    const ValueName& value,
    void* buffer,
    DWORD size,
    RegistryPath subKey) const {
  LSTATUS status = RegGetValue(
      handle_.get(),
      subKey,
      value.c_str(),
      RRF_RT_REG_BINARY,
      nullptr,
      buffer,
      &size);

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to get binary data: {}:{} size: {}",
            wideToMultibyteString(subKey),
            wideToMultibyteString(value),
            size));
  }
  return size;
}

void RegistryKey::setDWord(const ValueName& value, const DWORD data) const {
  LSTATUS status = RegSetValueExW(
      handle_.get(),
      value.c_str(),
      0,
      REG_DWORD,
      reinterpret_cast<const BYTE*>(&data),
      sizeof(DWORD));

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to set DWORD : {}", wideToMultibyteString(value)));
  }
}

void RegistryKey::setString(const ValueName& value, const std::wstring& data)
    const {
  // For string the size needs to include the size of terminating NULL character
  LSTATUS status = RegSetValueExW(
      handle_.get(),
      value.c_str(),
      0,
      REG_SZ,
      reinterpret_cast<const BYTE*>(data.data()),
      (data.size() + 1) * sizeof(wchar_t));

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to set String : {}", wideToMultibyteString(value)));
  }
}

void RegistryKey::setBinary(
    const ValueName& value,
    const void* data,
    size_t size) const {
  // For string the size needs to include the size of terminating NULL character
  LSTATUS status = RegSetValueExW(
      handle_.get(),
      value.c_str(),
      0,
      REG_BINARY,
      reinterpret_cast<const BYTE*>(data),
      size);

  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to set String : {}", wideToMultibyteString(value)));
  }
}

void RegistryKey::deleteValue(const ValueName& value) {
  LSTATUS status = RegDeleteValueW(handle_.get(), value.c_str());
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(
        status,
        folly::sformat(
            "Failed to delete Value : {}", wideToMultibyteString(value)));
  }
}

std::vector<std::pair<std::wstring, DWORD>> RegistryKey::enumerateValues() {
  DWORD numValues{};
  DWORD maxValueLength{};
  LSTATUS status = RegQueryInfoKey(
      handle_.get(),
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      nullptr,
      &numValues,
      &maxValueLength,
      nullptr,
      nullptr,
      nullptr);
  if (status != ERROR_SUCCESS) {
    throw makeWin32ErrorExplicit(status, "Failed to query value length");
  }

  // Add some buffer (& space for NULL character)
  maxValueLength += 32;
  auto nameBuffer = std::make_unique<wchar_t[]>(maxValueLength);
  std::vector<std::pair<std::wstring, DWORD>> valueEntries;

  for (DWORD index = 0; index < numValues;) {
    DWORD valueNameLen = maxValueLength;
    DWORD valueType{};
    status = RegEnumValue(
        handle_.get(),
        index,
        nameBuffer.get(),
        &valueNameLen,
        nullptr,
        &valueType,
        nullptr,
        nullptr);
    if (status == ERROR_MORE_DATA) {
      // We could only get here if a value was inserted between our call to
      // RegQueryInfoKey and RegEnumValue with bigger length. Reallocate out
      // buffer and continue.

      maxValueLength = valueNameLen + 32;
      nameBuffer = std::make_unique<wchar_t[]>(maxValueLength);
      continue;
    } else if (status == ERROR_NO_MORE_ITEMS) {
      // We could only get here if a value was deleted.
      break;
    }
    if (status != ERROR_SUCCESS) {
      throw makeWin32ErrorExplicit(status, "Failed to enumerate values");
    }

    valueEntries.emplace_back(std::make_pair(
        std::wstring{nameBuffer.get(), valueNameLen}, valueType));
    index++;
  }
  return valueEntries;
}

} // namespace eden
} // namespace facebook
