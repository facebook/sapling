# logtoprocess.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Send ui.log() data to a subprocess

This extension lets you specify a shell command per ui.log() event,
sending all remaining arguments to as environment variables to that command.

Each positional argument to the method results in a `MSG[N]` key in the
environment, starting at 1 (so `MSG1`, `MSG2`, etc.). Each keyword argument
is set as a `OPT_UPPERCASE_KEY` argument (so the key is uppercased, and
prefixed with `OPT_`).

So given a call `ui.log('foo', 'bar', 'baz', spam='eggs'), a script configured
for the `foo` event can expect an environment with `MSG1=bar`, `MSG2=baz`, and
`OPT_SPAM=eggs`.

Scripts are configured in the `[logtoprocess]` section, each key an event name.
For example::

  [logtoprocess]
  commandexception = echo "$MSG2$MSG3" > /var/log/mercurial_exceptions.log

would log the warning message and traceback of any failed command dispatch.

Scripts are run sychronously; they should exit ASAP. Preferably the command
should fork and disown to avoid slowing mercurial down.

"""

import os
import subprocess

from itertools import chain

def uisetup(ui):
    class logtoprocessui(ui.__class__):
        def log(self, event, *msg, **opts):
            """Map log events to external commands

            Arguments are passed on as environment variables.

            """
            script = ui.config('logtoprocess', event)
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
                env = dict(chain(os.environ.items(), msgpairs, optpairs))
                subprocess.call(script, shell=True, env=env)
            return super(logtoprocessui, self).log(event, *msg, **opts)

    # Replace the class for this instance and all clones created from it:
    ui.__class__ = logtoprocessui
