#
# Copyright 21 May 2005 - (c) 2005 Jake Edge <jake@edge2.net>
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

import cStringIO, zlib, tempfile, errno, os, sys, urllib, copy
from mercurial import util, streamclone, pushkey
from mercurial.node import bin, hex
from mercurial import changegroup as changegroupmod
from common import ErrorResponse, HTTP_OK, HTTP_NOT_FOUND, HTTP_SERVER_ERROR

# __all__ is populated with the allowed commands. Be sure to add to it if
# you're adding a new command, or the new command won't work.

__all__ = [
   'lookup', 'heads', 'branches', 'between', 'changegroup',
   'changegroupsubset', 'capabilities', 'unbundle', 'stream_out',
   'branchmap', 'pushkey', 'listkeys'
]

HGTYPE = 'application/mercurial-0.1'
