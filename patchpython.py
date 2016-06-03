# patchpython.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# Some code of this file is ported from Python 2.7.11 SocketServer.py, which
# has the following copyright:
#
#   Copyright (c) 2001, 2002, 2003, 2004, 2005, 2006
#   Python Software Foundation; All Rights Reserved
#
# and is being used under the following license:
#
#   PYTHON SOFTWARE FOUNDATION LICENSE VERSION 2
#   --------------------------------------------
#
#   1. This LICENSE AGREEMENT is between the Python Software Foundation
#   ("PSF"), and the Individual or Organization ("Licensee") accessing and
#   otherwise using this software ("Python") in source or binary form and
#   its associated documentation.
#
#   2. Subject to the terms and conditions of this License Agreement, PSF
#   hereby grants Licensee a nonexclusive, royalty-free, world-wide
#   license to reproduce, analyze, test, perform and/or display publicly,
#   prepare derivative works, distribute, and otherwise use Python
#   alone or in any derivative version, provided, however, that PSF's
#   License Agreement and PSF's notice of copyright, i.e., "Copyright (c)
#   2001, 2002, 2003, 2004, 2005, 2006 Python Software Foundation; All Rights
#   Reserved" are retained in Python alone or in any derivative version
#   prepared by Licensee.
#
#   3. In the event Licensee prepares a derivative work that is based on
#   or incorporates Python or any part thereof, and wants to make
#   the derivative work available to others as provided herein, then
#   Licensee hereby agrees to include in any such work a brief summary of
#   the changes made to Python.
#
#   4. PSF is making Python available to Licensee on an "AS IS"
#   basis.  PSF MAKES NO REPRESENTATIONS OR WARRANTIES, EXPRESS OR
#   IMPLIED.  BY WAY OF EXAMPLE, BUT NOT LIMITATION, PSF MAKES NO AND
#   DISCLAIMS ANY REPRESENTATION OR WARRANTY OF MERCHANTABILITY OR FITNESS
#   FOR ANY PARTICULAR PURPOSE OR THAT THE USE OF PYTHON WILL NOT
#   INFRINGE ANY THIRD PARTY RIGHTS.
#
#   5. PSF SHALL NOT BE LIABLE TO LICENSEE OR ANY OTHER USERS OF PYTHON
#   FOR ANY INCIDENTAL, SPECIAL, OR CONSEQUENTIAL DAMAGES OR LOSS AS
#   A RESULT OF MODIFYING, DISTRIBUTING, OR OTHERWISE USING PYTHON,
#   OR ANY DERIVATIVE THEREOF, EVEN IF ADVISED OF THE POSSIBILITY THEREOF.
#
#   6. This License Agreement will automatically terminate upon a material
#   breach of its terms and conditions.
#
#   7. Nothing in this License Agreement shall be deemed to create any
#   relationship of agency, partnership, or joint venture between PSF and
#   Licensee.  This License Agreement does not grant permission to use PSF
#   trademarks or trade name in a trademark sense to endorse or promote
#   products or services of Licensee, or any third party.
#
#   8. By copying, installing or otherwise using Python, Licensee
#   agrees to be bound by the terms and conditions of this License
#   Agreement.
"""patch python libraries

This extension patches some buggy python libraries.
It does not provide new commands or new features.
"""

import os
import select
import signal
import sys

from mercurial import util

def _patchsocketserver():
    """
    patch SocketServer to fix 2 issues

    - Python 2.6, critical race condition that can hang with CPU 100% forever
      https://bugs.python.org/issue21491
    - Python 2.6 and 2.7, zombie processes not reaped in time
      https://bugs.python.org/issue11109
    """
    if sys.version_info >= (3, 0):
        # Python 3 does not need patch
        return
    if sys.version_info < (2, 6):
        raise RuntimeError('Python < 2.6 is not supported')

    import SocketServer

    baseserver = SocketServer.BaseServer
    forkingmixin = SocketServer.ForkingMixIn

    # check if it looks okay to patch. Python 2.7.11 is known safe but
    # future Python 2 releases are unpredictable.
    shutdownnames = set(('_BaseServer__is_shut_down',
                         '_BaseServer__shutdown_request'))
    if not shutdownnames.issubset(baseserver.__init__.__code__.co_names) \
       or not util.safehasattr(baseserver, '_handle_request_noblock') \
       or not util.safehasattr(SocketServer, '_eintr_retry') \
       or not util.safehasattr(baseserver, 'finish_request') \
       or util.safehasattr(baseserver, 'serve_cleanup') \
       or 'active_children' not in \
            forkingmixin.process_request.__code__.co_names:
        return

    # patching SocketServer.BaseServer

    # adds a "serve_cleanup" method so ForkingMixIn can reap zombies
    def serve_forever(self, poll_interval=0.5):
        self._BaseServer__is_shut_down.clear()
        try:
            while not self._BaseServer__shutdown_request:
                r, w, e = SocketServer._eintr_retry(
                    select.select, [self], [], [], poll_interval)
                if self in r:
                    self._handle_request_noblock()
                self.serve_cleanup()
        finally:
            self._BaseServer__shutdown_request = False
            self._BaseServer__is_shut_down.set()

    def serve_cleanup(self):
        pass

    baseserver.serve_cleanup = serve_cleanup
    baseserver.serve_forever = serve_forever

    # "shutdown_request" is required but unavailable in Python 2.6
    if not util.safehasattr(baseserver, 'shutdown_request'):
        baseserver.shutdown_request = baseserver.close_request

    # patching SocketServer.ForkingMixIn

    # patch "process_request" to use Python 2.7 version since in Python 2.6,
    # self.active_children is a list but we want a set as in 2.7.
    def process_request(self, request, client_address):
        """Fork a new subprocess to process the request."""
        self.collect_children()
        pid = os.fork()
        if pid:
            # Parent process
            if self.active_children is None:
                self.active_children = set()
            self.active_children.add(pid)
            self.close_request(request) #close handle in parent process
            return
        else:
            # Child process.
            # This must never return, hence os._exit()!
            try:
                self.finish_request(request, client_address)
                self.shutdown_request(request)
                os._exit(0)
            except: # use re-raises to make check-code happy
                try:
                    self.handle_error(request, client_address)
                    self.shutdown_request(request)
                finally:
                    os._exit(1)

    # patch "collect_children" to use Python 2.7 version since Python 2.6 has a
    # critical race condition. See https://bugs.python.org/issue21491.
    def collect_children(self):
        """Internal routine to wait for children that have exited."""
        if self.active_children is None:
            return

        # If we're above the max number of children, wait and reap them until
        # we go back below threshold. Note that we use waitpid(-1) below to be
        # able to collect children in size(<defunct children>) syscalls instead
        # of size(<children>): the downside is that this might reap children
        # which we didn't spawn, which is why we only resort to this when we're
        # above max_children.
        while len(self.active_children) >= self.max_children:
            try:
                pid, _ = os.waitpid(-1, 0)
                self.active_children.discard(pid)
            except OSError as e:
                if e.errno == errno.ECHILD:
                    # we don't have any children, we're done
                    self.active_children.clear()
                elif e.errno != errno.EINTR:
                    break

        # Now reap all defunct children.
        for pid in self.active_children.copy():
            try:
                pid, _ = os.waitpid(pid, os.WNOHANG)
                # if the child hasn't exited yet, pid will be 0 and ignored by
                # discard() below
                self.active_children.discard(pid)
            except OSError as e:
                if e.errno == errno.ECHILD:
                    # someone else reaped it
                    self.active_children.discard(pid)

    forkingmixin.collect_children = collect_children
    forkingmixin.process_request = process_request
    forkingmixin.serve_cleanup = collect_children

_patchsocketserver()
