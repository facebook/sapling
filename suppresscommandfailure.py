# suppresscommandfailure.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""Suppress the commandfailure warning.

When a command breaks and raises an exception, two things happen:

* a warning message is generated
* Python exits with a traceback

This extension suppresses both the warning and the traceback, in the
expectation that other extensions handle the issue by other means.

"""

# When an exception is raised in a command:
#
# * Call ui.log with a warning message and the traceback.
# * Call ui.warn with the same warning message.
# * Re-raise the exception, exiting the Python process with a traceback.
#
# This extension suppresses the ui.warn call (by looking for the matching
# text), then sets the Python sys.excepthook to a no-op. This is preferred over
# exiting with sys.exit(1) at this point, to avoid interfering with other
# extensions.

import sys

def uisetup(ui):
    class suppresscommandfailureui(ui.__class__):
        def log(self, event, *msg, **opts):
            if event == 'commandexception':
                self._suppresswarning = msg[1]
            return super(suppresscommandfailureui, self).log(
                event, *msg, **opts)

        def warn(self, *msg, **opts):
            if msg and getattr(self, '_suppresswarning', None) == msg[0]:
                del self._suppresswarning
                sys.excepthook = lambda *args: None
                return
            return super(suppresscommandfailureui, self).warn(
                *msg, **opts)

    ui.__class__ = suppresscommandfailureui
