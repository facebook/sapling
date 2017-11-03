# logtoprocess.py - send ui.log() data to a subprocess
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""send ui.log() data to a subprocess (EXPERIMENTAL)

This extension lets you specify a shell command per ui.log() event,
sending all remaining arguments to as environment variables to that command.

Each positional argument to the method results in a `MSG[N]` key in the
environment, starting at 1 (so `MSG1`, `MSG2`, etc.). Each keyword argument
is set as a `OPT_UPPERCASE_KEY` variable (so the key is uppercased, and
prefixed with `OPT_`). The original event name is passed in the `EVENT`
environment variable, and the process ID of mercurial is given in `HGPID`.

So given a call `ui.log('foo', 'bar', 'baz', spam='eggs'), a script configured
for the `foo` event can expect an environment with `MSG1=bar`, `MSG2=baz`, and
`OPT_SPAM=eggs`.

Scripts are configured in the `[logtoprocess]` section, each key an event name.
For example::

  [logtoprocess]
  commandexception = echo "$MSG2$MSG3" > /var/log/mercurial_exceptions.log

would log the warning message and traceback of any failed command dispatch.

Scripts are run asynchronously as detached daemon processes; mercurial will
not ensure that they exit cleanly.

"""

from __future__ import absolute_import

import itertools
import os
import subprocess
import sys

from mercurial import (
    encoding,
    pycompat,
)

# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = 'ships-with-hg-core'

def uisetup(ui):
    if pycompat.iswindows:
        # no fork on Windows, but we can create a detached process
        # https://msdn.microsoft.com/en-us/library/windows/desktop/ms684863.aspx
        # No stdlib constant exists for this value
        DETACHED_PROCESS = 0x00000008
        _creationflags = DETACHED_PROCESS | subprocess.CREATE_NEW_PROCESS_GROUP

        def runshellcommand(script, env):
            # we can't use close_fds *and* redirect stdin. I'm not sure that we
            # need to because the detached process has no console connection.
            subprocess.Popen(
                script, shell=True, env=env, close_fds=True,
                creationflags=_creationflags)
    else:
        def runshellcommand(script, env):
            # double-fork to completely detach from the parent process
            # based on http://code.activestate.com/recipes/278731
            pid = os.fork()
            if pid:
                # parent
                return
            # subprocess.Popen() forks again, all we need to add is
            # flag the new process as a new session.
            if sys.version_info < (3, 2):
                newsession = {'preexec_fn': os.setsid}
            else:
                newsession = {'start_new_session': True}
            try:
                # connect stdin to devnull to make sure the subprocess can't
                # muck up that stream for mercurial.
                subprocess.Popen(
                    script, shell=True, stdin=open(os.devnull, 'r'), env=env,
                    close_fds=True, **newsession)
            finally:
                # mission accomplished, this child needs to exit and not
                # continue the hg process here.
                os._exit(0)

    class logtoprocessui(ui.__class__):
        def log(self, event, *msg, **opts):
            """Map log events to external commands

            Arguments are passed on as environment variables.

            """
            script = self.config('logtoprocess', event)
            if script:
                if msg:
                    # try to format the log message given the remaining
                    # arguments
                    try:
                        # Python string formatting with % either uses a
                        # dictionary *or* tuple, but not both. If we have
                        # keyword options, assume we need a mapping.
                        formatted = msg[0] % (opts or msg[1:])
                    except (TypeError, KeyError):
                        # Failed to apply the arguments, ignore
                        formatted = msg[0]
                    messages = (formatted,) + msg[1:]
                else:
                    messages = msg
                # positional arguments are listed as MSG[N] keys in the
                # environment
                msgpairs = (
                    ('MSG{0:d}'.format(i), str(m))
                    for i, m in enumerate(messages, 1))
                # keyword arguments get prefixed with OPT_ and uppercased
                optpairs = (
                    ('OPT_{0}'.format(key.upper()), str(value))
                    for key, value in opts.iteritems())
                env = dict(itertools.chain(encoding.environ.items(),
                                           msgpairs, optpairs),
                           EVENT=event, HGPID=str(os.getpid()))
                runshellcommand(script, env)
            return super(logtoprocessui, self).log(event, *msg, **opts)

    # Replace the class for this instance and all clones created from it:
    ui.__class__ = logtoprocessui
