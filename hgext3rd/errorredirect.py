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
import traceback

from mercurial import (
    dispatch,
    extensions,
)

def _handlecommandexception(orig, ui):
    script = ui.config('errorredirect', 'script')
    if not script:
        return orig(ui)

    warning = dispatch._exceptionwarning(ui)
    trace = traceback.format_exc()
    env = os.environ.copy()
    env['WARNING'] = warning
    env['TRACE'] = trace
    p = subprocess.Popen(script, shell=True, stdin=subprocess.PIPE, env=env)
    p.communicate(trace)
    return True # do not re-raise

def uisetup(ui):
    extensions.wrapfunction(dispatch, 'handlecommandexception',
                            _handlecommandexception)
