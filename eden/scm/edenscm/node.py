# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# node.py - basic nodeid manipulation for mercurial
#
# Copyright 2005, 2006 Olivia Mackall <olivia@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import binascii


# This ugly style has a noticeable effect in manifest parsing
bhex = binascii.hexlify
bbin = binascii.unhexlify


def bin(node):
    try:
        return bbin(node)
    except binascii.Error as e:
        raise TypeError(e)


hex = bytes.hex
nullrev = -1
nullid = b"\0" * 20
nullhex = hex(nullid)

# Phony node value to stand-in for new files in some uses of
# manifests.
newnodeid = b"!" * 20
addednodeid = (b"0" * 15) + b"added"
modifiednodeid = (b"0" * 12) + b"modified"

wdirnodes = {newnodeid, addednodeid, modifiednodeid}

# pseudo identifiers for working directory
# (they are experimental, so don't add too many dependencies on them)
wdirrev = 0x7FFFFFFF
wdirid = b"\xff" * 20
wdirhex = hex(wdirid)


def short(node):
    return hex(node[:6])
