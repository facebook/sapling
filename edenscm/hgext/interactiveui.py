# interactiveui.py: display information and allow for left/right control
#
# Copyright 2017 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import sys
import termios
import tty

from edenscm.mercurial import error
from edenscm.mercurial.i18n import _


def upline(n=1):
    w = sys.stdout
    # ANSI
    # ESC[#A : up # lines
    w.write("\033[%dA" % n)


def clearline(n=1):
    w = sys.stdout
    # ANSI
    # ESC[#A : up # lines
    # ESC[K : clear to end of line
    for i in range(n):
        w.write("\033[1A\033[K")


# From:
# https://github.com/pallets/click/blob/master/click/_termui_impl.py#L534
# As per licence:
# Copyright (c) 2014 by Armin Ronacher.
#
# Click uses parts of optparse written by Gregory P. Ward and maintained by
# the Python software foundation.  This is limited to code in the parser.py
# module:
#
# Copyright (c) 2001-2006 Gregory P. Ward.  All rights reserved.
# Copyright (c) 2002-2006 Python Software Foundation.  All rights reserved.
#
# Some rights reserved.
#
# Redistribution and use in source and binary forms, with or without
# modification, are permitted provided that the following conditions are
# met:
#
#    * Redistributions of source code must retain the above copyright
#      notice, this list of conditions and the following disclaimer.
#
#    * Redistributions in binary form must reproduce the above
#      copyright notice, this list of conditions and the following
#      disclaimer in the documentation and/or other materials provided
#      with the distribution.
#
#    * The names of the contributors may not be used to endorse or
#      promote products derived from this software without specific
#      prior written permission.
#
# THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
# "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
# LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
# A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
# OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
# SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
# LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
# DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
# THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
# (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
# OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

# Note: some changes have been made from the source code


def getchar(fd):
    if not os.isatty(fd):
        # TODO: figure out tests
        return None
    try:
        attr = termios.tcgetattr(fd)
        try:
            tty.setraw(fd)
            ch = os.read(fd, 32)
        finally:
            termios.tcsetattr(fd, termios.TCSADRAIN, attr)
    except termios.error:
        if ch is None:
            ch = ""
    if ch == "\x03":
        raise KeyboardInterrupt()
    if ch == "\x04":
        raise EOFError()
    return ch


# End of code from link


class viewframe(object):
    # framework for view
    def __init__(self, ui, repo, index):
        self.ui = ui
        self.repo = repo
        self.index = index
        ui.disablepager()
        repo.ui.disablepager()

    def render():
        # returns string to print
        pass

    def enter():
        # handle user keypress return
        pass

    def leftarrow():
        # handle user keypress left arrow
        pass

    def rightarrow():
        # handle user keypress right arrow
        pass

    def apress():
        # handle user keypress 'a'
        pass

    def dpress():
        # handle user keypress 'b'
        pass


def view(viewobj):
    done = False
    if viewobj.ui.pageractive:
        raise error.Abort(_("interactiveui doesn't work with pager"))
    # disable line wrapping
    # this is from curses.tigetstr('rmam')
    sys.stdout.write("\x1b[?7l")
    s = viewobj.render()
    sys.stdout.write(s)
    while not done:
        output = getchar(sys.stdin.fileno())
        if repr(output) == "'q'":
            done = True
            break
        if repr(output) == "'\\r'":
            # \r = return
            viewobj.enter()
            done = True
            break
        if repr(output) == "'\\x1b[C'":
            viewobj.rightarrow()
        if repr(output) == "'\\x1b[D'":
            viewobj.leftarrow()
        if repr(output) == "'a'":
            viewobj.apress()
        if repr(output) == "'d'":
            viewobj.dpress()
        linecount = s.count("\n")
        s = viewobj.render()
        newlinecount = s.count("\n")
        if newlinecount < linecount:
            clearline(linecount - newlinecount)
            upline(newlinecount)
        else:
            upline(linecount)
        slist = s.splitlines(True)
        sys.stdout.write("".join("\033[K" + line for line in slist))
        sys.stdout.flush()
    # re-enable line wrapping
    # this is from curses.tigetstr('smam')
    sys.stdout.write("\x1b[?7h")
    sys.stdout.flush()
