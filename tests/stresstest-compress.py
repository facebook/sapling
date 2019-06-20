#!/usr/bin/env python
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
from __future__ import absolute_import

import functools
import os
import struct
import timeit

import lz4 as pylz4
from edenscmnative.bindings import lz4 as rustlz4


def roundtrip(size=None):
    if size is None:
        size = struct.unpack(">I", b"\0" + os.urandom(3))[0]
    data = os.urandom(size)
    assert rustlz4.decompress(pylz4.compress(data)) == data
    assert pylz4.decompress(buffer(rustlz4.compress(data))) == data
    assert rustlz4.decompress(pylz4.compressHC(data)) == data
    assert pylz4.decompress(buffer(rustlz4.compresshc(data))) == data


def benchmark(data, hcdata=None):
    number = 100
    size = len(data)
    hcdata = hcdata or data

    for modname, func in [("pylz4", pylz4.compress), ("rustlz4", rustlz4.compress)]:
        timer = timeit.Timer(functools.partial(func, data))
        elapsed = timer.timeit(number=number)
        perf = size * number / elapsed / 1e6
        name = "%s.%s" % (modname, func.__name__)
        print("%24s: %8.2f MB/s" % (name, perf))

    for modname, func in [("pylz4", pylz4.compressHC), ("rustlz4", rustlz4.compresshc)]:
        timer = timeit.Timer(functools.partial(func, hcdata))
        elapsed = timer.timeit(number=number)
        perf = size * number / elapsed / 1e6
        name = "%s.%s" % (modname, func.__name__)
        print("%24s: %8.2f MB/s" % (name, perf))

    data = pylz4.compress(data)
    for modname, func in [("pylz4", pylz4.decompress), ("rustlz4", rustlz4.decompress)]:
        timer = timeit.Timer(functools.partial(func, data))
        elapsed = timer.timeit(number=number)
        perf = size * number / elapsed / 1e6
        name = "%s.%s" % (modname, func.__name__)
        print("%24s: %8.2f MB/s" % (name, perf))


if __name__ == "__main__":
    size = int(2e7)
    print("Benchmarking (easy to compress data)...")
    benchmark(b"\0" * size)
    print("Benchmarking (hard to compress data)...")
    benchmark(os.urandom(size), hcdata=os.urandom(size / 100))

    print("Testing roundtrips (Press Ctrl+C to stop)...")
    for i in range(256):
        roundtrip(i)
    tested = 0
    while True:
        tested += 1
        roundtrip(0)
        if tested % 100000 == 0:
            os.write(1, "\r  %d test cases passed" % tested)
