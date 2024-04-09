/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

/*
Use PEP 523 to insert a "pass through" function in the native stack to match
Python stacks. The "pass thorugh" function keeps the frame state in its stack
frame, so a native debugger may use it to read the Python stack, without
waiting for the GIL, or python-debuginfo to parse inline information.

The code is written in C instead of Rust becase:
- Related APIs are offically defined in `Python.h` and rapidly changing.
- `NO_OPT` does not seem to have a Rust equvilent.

To check if this compiles with multiple versions of Python, try:

    # from bindings/modules/pycext/
    PYTHON_SYS_EXECUTABLE=python3.8 cargo check
    PYTHON_SYS_EXECUTABLE=python3.10 cargo check
    PYTHON_SYS_EXECUTABLE=python3.11 cargo check

To learn examples about the APIs, check  cpython/Modules/_testinternalcapi.c.
*/

#define PY_SSIZE_T_CLEAN
#include <Python.h> // @manual=fbsource//third-party/python:python

#if defined(_WIN32)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif

// _PyInterpreterState_SetEvalFrameFunc is new in CPython 3.9.
#define HAS_SET_EVAL_FRAME_FUNC (PY_VERSION_HEX >= 0x03090000)

#if PY_VERSION_HEX >= 0x030b0000
// CPython 3.11 changed PyFrameObject* to _PyInterpreterFrame*.
#define PyFrame struct _PyInterpreterFrame
#else
#include <frameobject.h> // @manual=fbsource//third-party/python:python
#define PyFrame PyFrameObject
#endif

#if HAS_SET_EVAL_FRAME_FUNC

// Disable optimization like tail recursion, dead code elimination,
// so function args are pushed to stack.
#if defined(__clang__)
#define NO_OPT __attribute__((optnone))
#elif defined(__GNUC__) || defined(__GNUG__)
#define NO_OPT __attribute__((optimize("O0")))
#else
#define NO_OPT
#endif

#if defined(_MSC_VER)
#pragma optimize("", off)
#endif

EXPORT PyObject* NO_OPT
Sapling_PyEvalFrame(PyThreadState* tstate, PyFrame* f, int exc) {
  return _PyEval_EvalFrameDefault(tstate, f, exc);
}

#if defined(_MSC_VER)
#pragma optimize("", on)
#endif

#endif // HAS_SET_EVAL_FRAME_FUNC

/**
 * Update the "EvalFrame" function to go through pass_through_eval_frame to
 * track Python function names in the native stack. Intended to be called by
 * cpython bindings in Rust.
 */
void sapling_cext_evalframe_set_pass_through(unsigned char enabled) {
#if HAS_SET_EVAL_FRAME_FUNC
  _PyInterpreterState_SetEvalFrameFunc(
      PyInterpreterState_Get(),
      enabled ? Sapling_PyEvalFrame : _PyEval_EvalFrameDefault);
#endif
}

/**
 * Resolve a PyFrame to a "name at path:line".
 * Intended to be called by a debugger like lldb. Not thread safe.
 */
EXPORT const char* sapling_cext_evalframe_resolve_frame(size_t address) {
  PyFrame* f = (PyFrame*)address;
  static char buf[4096] = {0};
  if (!f) {
    return buf;
  }
  memset(buf, 0, sizeof buf);
  // 3.12: f is _PyInterpreterFrame. Can be accessed via PyUnstable APIs.
  // 3.11: f is _PyInterpreterFrame. Need Py_BUILD_CORE_MODULE to access.
  // See also
  // https://github.com/python/cpython/issues/91006#issuecomment-1093945542
#if PY_VERSION_HEX >= 0x03090000 && PY_VERSION_HEX < 0x030b0000
  // 3.9-3.10: f is PyFrameObject* and can be read by PyFrame APIs.
  PyCodeObject* code = NULL;
  if (!PyFrame_Check(f)) {
    goto out;
  }
  code = PyFrame_GetCode(f);
  if (code == NULL) {
    goto out;
  }
  PyObject* filename_obj = code->co_filename;
  PyObject* name_obj = code->co_name;
  if (!filename_obj || !name_obj || !PyUnicode_Check(filename_obj) ||
      !PyUnicode_Check(name_obj)) {
    goto out;
  }
  const char* filename = PyUnicode_AsUTF8(filename_obj);
  const char* name = PyUnicode_AsUTF8(name_obj);
  if (filename == NULL || name == NULL) {
    goto out;
  }
  int line_no = PyFrame_GetLineNumber(f);
  snprintf(buf, (sizeof buf) - 1, "%s at %s:%d", name, filename, line_no);
out:
  Py_XDECREF(code);
#endif
  return buf;
}
