#!/usr/bin/env python
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# cacheclient.py - example cache client implementation

# The remotefilelog extension can optionally use a caching layer to serve
# file revision requests. This is an example implementation that uses
# the python-memcached library: https://pypi.python.org/pypi/python-memcached/
# A better implementation would make all of the requests non-blocking.
from __future__ import absolute_import

import os
import sys

# pyre-fixme[21]: Could not find `memcache`.
import memcache


stdin = sys.stdin
stdout = sys.stdout
stderr = sys.stderr

mc = None
keyprefix = None
cachepath = None

# Max number of keys per request
batchsize = 1000

# Max value size per key (in bytes)
maxsize = 512 * 1024


def readfile(path):
    f = open(path, "r")
    try:
        return f.read()
    finally:
        f.close()


def writefile(path, content):
    dirname = os.path.dirname(path)
    if not os.path.exists(dirname):
        os.makedirs(dirname)

    f = open(path, "w")
    try:
        f.write(content)
    finally:
        f.close()


def compress(value):
    # Real world implementations will want to compress values.
    # Insert your favorite compression here, ex:
    # return lz4wrapper.lzcompresshc(value)
    return value


def decompress(value):
    # Real world implementations will want to compress values.
    # Insert your favorite compression here, ex:
    # return lz4wrapper.lz4decompress(value)
    return value


def generateKey(id):
    return keyprefix + id


def generateId(key):
    return key[len(keyprefix) :]


def getKeys():
    raw = stdin.readline()[:-1]
    keycount = int(raw)

    keys = []
    for i in range(keycount):
        id = stdin.readline()[:-1]
        keys.append(generateKey(id))

    results = mc.get_multi(keys)

    hits = 0
    for i, key in enumerate(keys):
        value = results.get(key)
        id = generateId(key)
        # On hit, write to disk
        if value:
            # Integer hit indicates a large file
            if isinstance(value, int):
                largekeys = list([key + str(i) for i in range(value)])
                largevalues = mc.get_multi(largekeys)
                if len(largevalues) == value:
                    value = ""
                    for largekey in largekeys:
                        value += largevalues[largekey]
                else:
                    # A chunk is missing, give up
                    stdout.write(id + "\n")
                    stdout.flush()
                    continue
            path = os.path.join(cachepath, id)
            value = decompress(value)
            writefile(path, value)
            hits += 1
        else:
            # On miss, report to caller
            stdout.write(id + "\n")
            stdout.flush()

        if i % 500 == 0:
            stdout.write("_hits_%s_\n" % hits)
            stdout.flush()

    # done signal
    stdout.write("0\n")
    stdout.flush()


def setKeys():
    raw = stdin.readline()[:-1]
    keycount = int(raw)

    values = {}
    for i in range(keycount):
        id = stdin.readline()[:-1]
        path = os.path.join(cachepath, id)

        value = readfile(path)
        value = compress(value)

        key = generateKey(id)
        if len(value) > maxsize:
            # split up large files
            start = 0
            i = 0
            while start < len(value):
                end = min(len(value), start + maxsize)
                values[key + str(i)] = value[start:end]
                start += maxsize
                i += 1

            # Large files are stored as an integer representing how many
            # chunks it's broken into.
            value = i

        values[key] = value

        if len(values) == batchsize:
            mc.set_multi(values)
            values = {}

    if values:
        mc.set_multi(values)


def main(argv=None):
    """
    remotefilelog uses this cacheclient by setting it in the repo config:

    [remotefilelog]
    cacheprocess = cacheclient <ip address:port> <memcache prefix>

    When memcache requests need to be made, it will execute this process
    with the following arguments:

    cacheclient <ip address:port> <memcache prefix><internal prefix> <cachepath>

    Communication happens via stdin and stdout. To make a get request,
    the following is written to stdin:

    get\n
    <key count>\n
    <key1>\n
    <key...>\n
    <keyN>\n

    The results of any cache hits will be written directly to <cachepath>/<key>.
    Any cache misses will be written to stdout in the form <key>\n. Once all
    hits and misses are finished 0\n will be written to stdout to signal
    completion.

    During the request, progress may be reported via stdout with the format
    _hits_###_\n where ### is an integer representing the number of hits so
    far. remotefilelog uses this to display a progress bar.

    A single cacheclient process may be used for multiple requests (though
    not in parallel), so it stays open until it receives exit\n via stdin.

    """
    if argv is None:
        argv = sys.argv

    global cachepath
    global keyprefix
    global mc

    ip = argv[1]
    keyprefix = argv[2]
    cachepath = argv[3]

    mc = memcache.Client([ip], debug=0)

    while True:
        cmd = stdin.readline()[:-1]
        if cmd == "get":
            getKeys()
        elif cmd == "set":
            setKeys()
        elif cmd == "exit":
            return 0
        else:
            stderr.write("Invalid Command %s\n" % cmd)
            return 1


if __name__ == "__main__":
    sys.exit(main())
