# profiling.py - simple timing based SCM profiling
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""simple profiling for total and interactive time

With this extension enabled, the execution of mercurial will be timed,
and wrappers will be inserted around UI and interactive calls.  This
can give a better representation of time spent actually doing useful
work.  Upon exit, this extension will issue a ui.log call with a dictionary
containing interactive_time and internal_time, which are the time excluded
and total measured run time, respectively.  This ui.log call can be
intercepted or redirected by another extension.

We only track interactive time on the primary thread.
"""

import contextlib, threading
from mercurial import profiling, extensions
from mercurial import ui as uimod
from time import time as clock

wrappeduifuncs = ['write', 'write_err', '_readline', 'flush', 'system']

class profiletime(object):
    def __init__(self, ui):
        self.ui = ui
        self.threadid = None

    def __enter__(self):
        self.threadid = threading.current_thread().ident
        self.nesting_level = 0
        self.suspend_time = 0
        self.total_suspended_time = 0
        self.start_time = clock()

    def __exit__(self, exc_type, exc_value, traceback):
        total_time = clock() - self.start_time
        kwargs = {'internal_time': int(total_time * 1000),
                  'interactive_time': int(self.total_suspended_time * 1000)}
        self.ui.log('profiletime', '', **kwargs)

    def suspend(self):
        if threading.current_thread().ident == self.threadid:
            if self.nesting_level == 0:
                self.suspend_time = clock()
            self.nesting_level += 1

    def resume(self):
        if threading.current_thread().ident == self.threadid:
            self.nesting_level -= 1
            if not self.nesting_level:
                elapsed = clock() - self.suspend_time
                self.total_suspended_time += elapsed

def uisetup(ui):
    profilerctx = profiletime(ui)

    @contextlib.contextmanager
    def profile(orig, *args, **kwargs):
        with profilerctx:
            yield orig(*args, **kwargs)

    extensions.wrapfunction(profiling, 'maybeprofile', profile)

    # Wrap functions on the ui module directly
    wrapfns(uimod.ui, wrappeduifuncs, profilerctx.suspend, profilerctx.resume)

def wrapfns(ui, fns, pre, post):
    """Wrap listed functions of a class with pre and post calls"""

    def wrapper(orig, *args, **kwargs):
        pre()
        try:
            return orig(*args, **kwargs)
        finally:
            post()

    for fn in fns:
        extensions.wrapfunction(ui, fn, wrapper)
