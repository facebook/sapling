"""
node.py - basic nodeid manipulation for mercurial

Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from demandload import demandload
demandload(globals(), "binascii")

nullrev = -1
nullid = "\0" * 20

def hex(node):
    return binascii.hexlify(node)

def bin(node):
    return binascii.unhexlify(node)

def short(node):
    return hex(node[:6])
