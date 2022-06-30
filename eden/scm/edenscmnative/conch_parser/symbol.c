/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Keep the PyInit_conch_parser symbol defined in Rust alive across various
// buck build modes.
#include <Python.h>

// @dep=//eden/scm/edenscmnative/conch_parser:rust_conch_parser
extern PyMODINIT_FUNC PyInit_conch_parser();

PyMODINIT_FUNC PyInit_conch_parser_(void) {
  return PyInit_conch_parser();
}
