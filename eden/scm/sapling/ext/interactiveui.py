# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# interactiveui.py: display information and allow for left/right control

from __future__ import absolute_import

import os
import sys
from enum import Enum
from typing import Union

from sapling import error, pycompat, scmutil, util
from sapling.i18n import _, _x


if not pycompat.iswindows:
    import termios
    import tty


def clearscreen(out):
    if util.istest():
        out.write(_x("===== Screen Refresh =====\n"))
    else:
        out.write(_x("\033[2J"))  # clear screen
        out.write(_x("\033[;H"))  # move cursor


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


def getchar() -> Union[None, bytes, str]:
    fd = sys.stdin.fileno()
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
        # pyre-fixme[61]: `ch` is undefined, or not always defined.
        if ch is None:
            ch = ""
    # pyre-fixme[61]: `ch` is undefined, or not always defined.
    if ch == b"\x03" or ch == b"\x04":
        return None
    # pyre-fixme[61]: `ch` is undefined, or not always defined.
    return ch


# End of code from link


class Alignment(Enum):
    top = 1
    bottom = 2


class viewframe:
    # Useful Keycode Constants
    KEY_J = b"j"
    KEY_K = b"k"
    KEY_Q = b"q"
    KEY_R = b"r"
    KEY_S = b"s"
    KEY_RETURN = b"\r"
    KEY_UP = b"\x1b[A"
    KEY_DOWN = b"\x1b[B"
    KEY_RIGHT = b"\x1b[C"
    KEY_LEFT = b"\x1b[D"

    # framework for view
    def __init__(self, ui, repo):
        self.ui = ui
        self.repo = repo
        self.status = ""
        self._active = True
        ui.disablepager()
        repo.ui.disablepager()

    def render(self):
        # returns list of strings (rows) to print, and an optional tuple of (index, position)
        # Ensures that the row `index` is aligned to the `position` side of the screen if the list is longer than the screen height
        pass

    def handlekeypress(self, key):
        # handle user keypress
        pass

    def finish(self):
        # End interactive session
        self._active = False


def _write_output(viewobj):
    screensize = scmutil.termsize(viewobj.ui)[1]
    statuslines = viewobj.status.splitlines()
    statussize = len(statuslines)
    logsize = screensize - statussize
    clearscreen(viewobj.ui)
    lines, alignment = viewobj.render()
    if alignment is not None and len(lines) > logsize:
        index, direction = alignment
        if direction == Alignment.top:
            end = min(len(lines), index + logsize)
            start = min(index, end - logsize)
        elif direction == Alignment.bottom:
            start = max(0, index - logsize)
            end = max(index, start + logsize)
        lines = lines[start:end]

    lines += statuslines
    if util.istest():
        viewobj.ui.write("\n".join(lines))
    else:
        viewobj.ui.write("\n".join("\r" + line for line in lines))
    viewobj.ui.flush()


def view(viewobj, readinput=getchar) -> None:
    if pycompat.iswindows:
        raise error.Abort(_("interactive UI does not support Windows"))
    if viewobj.ui.pageractive:
        raise error.Abort(_("interactiveui doesn't work with pager"))
    # Enter alternate screen
    # TODO: Investigate portability - may only work for xterm
    if not util.istest():
        viewobj.ui.write(_x("\033[?1049h\033[H"))
        # disable line wrapping
        # this is from curses.tigetstr('rmam')
        viewobj.ui.write(_x("\x1b[?7l"))
        viewobj.ui.write(_x("\033[?25l"))  # hide cursor
    try:
        while viewobj._active:
            _write_output(viewobj)
            output = readinput()
            if output is None:
                break
            viewobj.handlekeypress(output)
    finally:
        if not util.istest():
            viewobj.ui.write(_x("\033[?25h"))  # show cursor
            # re-enable line wrapping
            # this is from curses.tigetstr('smam')
            viewobj.ui.write(_x("\x1b[?7h"))
            viewobj.ui.flush()
            # Exit alternate screen
            viewobj.ui.write(_x("\033[?1049l"))
