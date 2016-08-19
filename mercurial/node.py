# node.py - basic nodeid manipulation for mercurial
#
# Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import binascii

# This ugly style has a noticeable effect in manifest parsing
hex = binascii.hexlify
bin = binascii.unhexlify

nullrev = -1
nullid = b"\0" * 20
nullhex = hex(nullid)

# Phony node value to stand-in for new files in some uses of
# manifests.
newnodeid = '!' * 20
addednodeid = ('0' * 15) + 'added'
modifiednodeid = ('0' * 12) + 'modified'

wdirnodes = {newnodeid, addednodeid, modifiednodeid}

# pseudo identifiers for working directory
# (they are experimental, so don't add too many dependencies on them)
wdirrev = 0x7fffffff
wdirid = b"\xff" * 20
wdirhex = hex(wdirid)

def short(node):
    return hex(node[:6])
