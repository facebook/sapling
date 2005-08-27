"""
node.py - basic nodeid manipulation for mercurial

Copyright 2005 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

import sha, binascii

nullid = "\0" * 20

def hex(node):
    return binascii.hexlify(node)

def bin(node):
    return binascii.unhexlify(node)

def short(node):
    return hex(node[:6])

def hash(text, p1, p2):
    """generate a hash from the given text and its parent hashes

    This hash combines both the current file contents and its history
    in a manner that makes it easy to distinguish nodes with the same
    content in the revision graph.
    """
    l = [p1, p2]
    l.sort()
    s = sha.new(l[0])
    s.update(l[1])
    s.update(text)
    return s.digest()

