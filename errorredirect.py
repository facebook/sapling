# errorredirect.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""redirect error message

Redirect error message, the stack trace, of an uncaught exception to
a custom shell script. This is useful for further handling the error,
for example posting it to a support group and logging it somewhere.

The config option errorredirect.script is the shell script to execute.
If it's empty, the extension will do nothing and fallback to the old
behavior.

Two environment variables are set: TRACE is the stack trace, which
is the same as piped content. WARNING is the warning message, which
usually contains contact message and software versions, etc.

Examples::

  [errorredirect]
  script = tee hgerr.log && echo 'Error written to hgerr.log'

  [errorredirect]
  script = echo "$WARNING$TRACE" >&2

  [errorredirect]
  script = (echo "$WARNING"; cat) | cat >&2
"""

import os
import subprocess
import sys


def wrapui(ui):
    class errorredirectui(ui.__class__):
        def errorredirect(self, content, env):
            script = self.config('errorredirect', 'script')
            if not script:
                return
            p = subprocess.Popen(script, shell=True, stdin=subprocess.PIPE,
                                 env=dict(os.environ.items() + env.items()))
            p.communicate(content)
            # prevent hg from printing the stack trace
            sys.exit(1)

        def log(self, event, *msg, **opts):
            if event == 'commandexception':
                warning, traceback = msg[-2:]
                self.errorredirect(traceback,
                                   {'WARNING': warning, 'TRACE': traceback})
            return super(errorredirectui, self).log(event, *msg, **opts)

    ui.__class__ = errorredirectui

def uisetup(ui):
    wrapui(ui)
