# errorredirect.py
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""redirect error message

Redirect error message, the stack trace, of an uncaught exception to
a custom shell script. This is useful for futher handling the error,
for example posting it to a support group and logging it somewhere.

The config option errorredirect.script is the shell script to execute.
If it's empty, the extension will do nothing and fallback to the old
behavior.

Note: the error message passed to the script contains only the stack
trace without the contact support header. Several environment variables
are set so it's easy to print similiar notice, see the second example.

Examples::

  [errorredirect]
  script = tee hgerr.log && echo 'Error written to hgerr.log'

  [errorredirect]
  script = (
      echo '**' unknown exception encountered, please report by visiting
      echo '**' ${CONTACT:-https://mercurial-scm.org/wiki/BugTracker}
      echo '**' Python $PYTHONVERSION
      echo '**' Mercurial Distributed SCM "(version $HGVERSION)"
      echo '**' Extensions loaded: $EXTENSIONS
      cat) | cat >&2
"""

import os
import subprocess
import sys
from mercurial import extensions, util


def wrapui(ui):
    class errorredirectui(ui.__class__):
        def errorredirect(self, content):
            script = self.config('errorredirect', 'script')
            if not script:
                return
            env = {
              'CONTACT': ui.config('ui', 'supportcontact', ''),
              'EXTENSIONS': ', '.join([x[0] for x in extensions.extensions()]),
              'HGVERSION': util.version(),
              'PYTHONVERSION': sys.version,
            }
            p = subprocess.Popen(script, shell=True, stdin=subprocess.PIPE,
                                 env=dict(os.environ.items() + env.items()))
            p.communicate(content)
            # prevent hg from printing the stack trace
            sys.exit(1)

        def log(self, event, *msg, **opts):
            if event == 'commandexception':
                # msg = [header, traceback]
                tracestr = msg[len(msg) - 1]
                self.errorredirect(tracestr)
            return super(errorredirectui, self).log(event, *msg, **opts)

    ui.__class__ = errorredirectui

def uisetup(ui):
    wrapui(ui)
