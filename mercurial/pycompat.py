# pycompat.py - portability shim for python 3
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""Mercurial portability shim for python 3.

This contains aliases to hide python version-specific details from the core.
"""

from __future__ import absolute_import

try:
    import Queue as _queue
    _queue.Queue
except ImportError:
    import queue as _queue
empty = _queue.Empty
queue = _queue.Queue

try:
    xrange
except NameError:
    import builtins
    builtins.xrange = range
