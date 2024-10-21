/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#define PY_SSIZE_T_CLEAN
#include <Python.h>

#include <Windows.h> // @manual
#include <libloaderapi.h> // @manual
#include <ntstatus.h> // @manual
#include <winternl.h> // @manual

#include <cstdlib>
#include <cstring>
#include <iostream>
#include <memory>
#include <string_view>

/*
 * Dynamically loaded functionality from ntdll.
 */

typedef struct _FILE_NAMES_INFORMATION {
  ULONG NextEntryOffset;
  ULONG FileIndex;
  ULONG FileNameLength;
  WCHAR FileName[1];
} FILE_NAMES_INFORMATION, *PFILE_NAMES_INFORMATION;

typedef enum {
  FileNamesInformation = 12,
} FILE_INFORMATION_CLASS_,
    *PFILE_INFORMATION_CLASS_;

// https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/nf-ntifs-ntquerydirectoryfileex
typedef NTSTATUS(WINAPI* NTQUERYDIRECTORYFILEEX)(
    _In_ HANDLE FileHandle,
    _In_opt_ HANDLE Event,
    _In_opt_ PIO_APC_ROUTINE ApcRoutine,
    _In_opt_ PVOID ApcContext,
    _Out_ PIO_STATUS_BLOCK IoStatusBlock,
    _Out_ PVOID FileInformation,
    _In_ ULONG Length,
    FILE_INFORMATION_CLASS_ FileInformationClass,
    _In_ ULONG QueryFlags,
    _In_opt_ PUNICODE_STRING FileName);

static NTQUERYDIRECTORYFILEEX NtQueryDirectoryFileEx = nullptr;

typedef void(NTAPI* RTLINITUNICODESTRING)(
    _Out_ PUNICODE_STRING DestinationString,
    _In_opt_ PCWSTR SourceString);

static RTLINITUNICODESTRING _RtlInitUnicodeString = nullptr;

/**
 * A Python type that owns a Win32 HANDLE.
 *
 * Closes the handle on destruction.
 */
typedef struct {
  PyObject_HEAD HANDLE h;
} HandleObject;

static PyTypeObject HandleType{PyVarObject_HEAD_INIT(nullptr, 0)};

static void Handle_dealloc(PyObject* self) {
  CloseHandle(reinterpret_cast<HandleObject*>(self)->h);
}

struct PyMemDeleter {
  void operator()(void* ptr) {
    if (ptr != nullptr) {
      PyMem_Free(ptr);
    }
  }
};

/**
 * A pointer to type T that needs to be freed with PyMem_Free.
 */
template <typename T>
using PyMemPtr = std::unique_ptr<std::remove_pointer_t<T>, PyMemDeleter>;

template <typename T>
struct PyRefDecrementer {
  void operator()(T* ptr) {
    Py_XDECREF(reinterpret_cast<PyObject*>(ptr));
  }
};

/**
 * A reference to a PyObject.
 *
 * Ensures the reference count gets decremented when going out of scope.
 */
template <typename T = PyObject>
using PyRef = std::unique_ptr<std::remove_pointer_t<T>, PyRefDecrementer<T>>;

template <typename T = PyObject>
static PyRef<T> take_pyref_ownership(T* obj) {
  Py_XINCREF(reinterpret_cast<PyObject*>(obj));
  return PyRef<T>(obj);
}

static PyObject* open_directory_handle(PyObject* self, PyObject* args) {
  PyObject* path;

  if (!PyArg_ParseTuple(args, "U:open_directory_handle", &path)) {
    return nullptr;
  }

  // Pass nullptr to size parameter so that we intentionally fail on strings
  // containing null characters.
  auto pathWstr = PyMemPtr<wchar_t>(PyUnicode_AsWideCharString(path, nullptr));
  if (pathWstr.get() == nullptr) {
    return nullptr;
  }

  auto result = PyRef<HandleObject>(PyObject_New(HandleObject, &HandleType));
  result->h = CreateFileW(
      pathWstr.get(),
      GENERIC_READ,
      FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
      nullptr,
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS,
      nullptr);

  if (result->h == INVALID_HANDLE_VALUE) {
    const char* error = "Unknown";
    FormatMessage(
        FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_ALLOCATE_BUFFER |
            FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr,
        GetLastError(),
        MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
        reinterpret_cast<LPSTR>(&error),
        0,
        nullptr);
    PyErr_Format(PyExc_RuntimeError, "Error from CreateFileW: %s", error);
    return nullptr;
  }

  return reinterpret_cast<PyObject*>(result.release());
}

static PyObject* open_file_handle(PyObject* self, PyObject* args) {
  PyObject* path;
  char openFlag;
  DWORD dwDesiredAccess;
  DWORD dwShareMode;

  if (!PyArg_ParseTuple(
          args, "Uci:open_file_handle", &path, &openFlag, &dwShareMode)) {
    return nullptr;
  }

  // Pass nullptr to size parameter so that we intentionally fail on strings
  // containing null characters.
  auto pathWstr = PyMemPtr<wchar_t>(PyUnicode_AsWideCharString(path, nullptr));
  if (pathWstr.get() == nullptr) {
    return nullptr;
  }

  // Due to python using 0x80000000 as a 64b signed int, we need to use
  // indirection
  if (openFlag == 'r') {
    dwDesiredAccess = GENERIC_READ;
  } else if (openFlag == 'w') {
    dwDesiredAccess = GENERIC_WRITE;
  } else if (openFlag == '+') {
    dwDesiredAccess = GENERIC_READ | GENERIC_WRITE;
  } else {
    return nullptr;
  }

  auto result = PyRef<HandleObject>(PyObject_New(HandleObject, &HandleType));
  // https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew
  result->h = CreateFileW(
      pathWstr.get(),
      dwDesiredAccess,
      dwShareMode,
      /*lpSecurityAttributes=*/nullptr,
      OPEN_EXISTING,
      FILE_FLAG_BACKUP_SEMANTICS,
      /*hTemplateFile=*/nullptr);

  if (result->h == INVALID_HANDLE_VALUE) {
    const char* error = "Unknown";
    FormatMessage(
        FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_ALLOCATE_BUFFER |
            FORMAT_MESSAGE_IGNORE_INSERTS,
        nullptr,
        GetLastError(),
        MAKELANGID(LANG_NEUTRAL, SUBLANG_DEFAULT),
        reinterpret_cast<LPSTR>(&error),
        0,
        nullptr);
    PyErr_Format(PyExc_RuntimeError, "Error from CreateFileW: %s", error);
    return nullptr;
  }

  return reinterpret_cast<PyObject*>(result.release());
}

static PyObject* query_directory_file_ex(PyObject* self, PyObject* args) {
  PyObject* handle;
  Py_ssize_t bufferSize;
  unsigned int queryFlags;
  PyObject* fileName;

  constexpr Py_ssize_t bufferMax = 16 * 1024;
  alignas(4) char buffer[bufferMax];
  PyMemPtr<wchar_t> fileNameWstr{nullptr};
  UNICODE_STRING fileNameUniStr;

  if (!PyArg_ParseTuple(
          args,
          "OnIO:query_directory_file_ex",
          &handle,
          &bufferSize,
          &queryFlags,
          &fileName)) {
    return nullptr;
  }

  if (PyObject_TypeCheck(handle, &HandleType) == 0) {
    PyErr_SetString(
        PyExc_ValueError, "Expected Handle for first handle argument");
    return nullptr;
  }

  if (bufferSize > bufferMax) {
    PyErr_Format(
        PyExc_ValueError,
        "bufferSize %lu greater than maximum allowed value %lu",
        bufferSize,
        bufferMax);
    return nullptr;
  }

  if (PyObject_TypeCheck(fileName, &PyUnicode_Type) == 0 &&
      fileName != Py_None) {
    PyErr_SetString(
        PyExc_ValueError, "Expected str or None for fileName argument");
    return nullptr;
  }

  if (fileName != Py_None) {
    fileNameWstr.reset(PyUnicode_AsWideCharString(fileName, nullptr));
    if (fileNameWstr.get() == nullptr) {
      return nullptr;
    }
    _RtlInitUnicodeString(&fileNameUniStr, fileNameWstr.get());
  }

  auto result = PyRef<>(PyList_New(0));

  IO_STATUS_BLOCK ioStatus;
  auto ret = NtQueryDirectoryFileEx(
      reinterpret_cast<HandleObject*>(handle)->h,
      /* Event= */ nullptr,
      /* ApcRoutine= */ nullptr,
      /* ApcContext= */ nullptr,
      &ioStatus,
      &buffer,
      bufferSize,
      FileNamesInformation,
      queryFlags,
      fileName != Py_None ? &fileNameUniStr : nullptr);
  if (ret == STATUS_NO_MORE_FILES) {
    return result.release();
  } else if (ret != STATUS_SUCCESS) {
    PyErr_Format(
        PyExc_RuntimeError,
        "Error result from NtQueryDirectoryFileEx: %x",
        ret);
    return nullptr;
  }

  std::size_t offset = 0;

  // After a successful query, the IO_STATUS_BLOCK's Information field contains
  // the number of bytes written to the buffer.
  while (offset < ioStatus.Information) {
    auto ent = reinterpret_cast<FILE_NAMES_INFORMATION*>(buffer + offset);

    auto filename = PyRef<>(PyUnicode_FromWideChar(
        ent->FileName, ent->FileNameLength / sizeof(ent->FileName[0])));
    if (filename.get() == nullptr) {
      return nullptr;
    }

    if (PyList_Append(result.get(), filename.release()) != 0) {
      return nullptr;
    }

    if (ent->NextEntryOffset != 0) {
      offset += ent->NextEntryOffset;
    } else {
      break;
    }
  }
  return result.release();
}

static PyObject* get_directory_entry_size(PyObject* self, PyObject* args) {
  if (!PyArg_ParseTuple(args, ":get_directory_entry_size")) {
    return nullptr;
  }
  return PyLong_FromSize_t(sizeof(FILE_NAMES_INFORMATION));
}

static PyMethodDef methods[] = {
    {"open_directory_handle",
     open_directory_handle,
     METH_VARARGS,
     "Opens a Handle to a named directory\n"},
    {"open_file_handle",
     open_file_handle,
     METH_VARARGS,
     "Opens a Handle to a named file\n"},
    {"query_directory_file_ex",
     query_directory_file_ex,
     METH_VARARGS,
     "Wrapper for NtQueryDirectoryFileEx\n"},
    {"get_directory_entry_size",
     get_directory_entry_size,
     METH_VARARGS,
     "Returns size of the directory entry type written to the buffer by query_directory_file_ex\n"},
    {nullptr, nullptr, 0, nullptr}};

static char ntapi_doc[] = "NT API wrappers for testing";

static struct PyModuleDef ntapi_module =
    {PyModuleDef_HEAD_INIT, "ntapi", ntapi_doc, -1, methods};

PyMODINIT_FUNC PyInit_ntapi(void) {
  auto* ntdll = GetModuleHandleA("ntdll");

  NtQueryDirectoryFileEx = reinterpret_cast<NTQUERYDIRECTORYFILEEX>(
      GetProcAddress(ntdll, "NtQueryDirectoryFileEx"));
  if (NtQueryDirectoryFileEx == nullptr) {
    return nullptr;
  }

  _RtlInitUnicodeString = reinterpret_cast<RTLINITUNICODESTRING>(
      GetProcAddress(ntdll, "RtlInitUnicodeString"));
  if (_RtlInitUnicodeString == nullptr) {
    return nullptr;
  }

  HandleType.tp_name = "eden.integration.lib.Handle";
  HandleType.tp_basicsize = sizeof(HandleObject);
  HandleType.tp_itemsize = 0;
  HandleType.tp_dealloc = Handle_dealloc;
  HandleType.tp_flags = Py_TPFLAGS_DEFAULT;
  HandleType.tp_doc = PyDoc_STR("Win32 Handle");
  HandleType.tp_new = PyType_GenericNew;
  if (PyType_Ready(&HandleType) < 0) {
    return nullptr;
  }
  auto handleTypeObj =
      take_pyref_ownership<>(reinterpret_cast<PyObject*>(&HandleType));

  auto m = PyRef<>(PyModule_Create(&ntapi_module));
  if (m.get() == nullptr) {
    return nullptr;
  }

  if (PyModule_AddObject(m.get(), "Handle", handleTypeObj.get()) < 0) {
    return nullptr;
  }
  return m.release();
}
