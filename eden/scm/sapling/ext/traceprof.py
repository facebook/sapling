# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import contextlib

import bindings


@contextlib.contextmanager
def profile(_ui, _fp, _section="profiling"):
    t = bindings.cext.TraceProf()
    try:
        with t:
            yield
    finally:
        t.report()
