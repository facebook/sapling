# inhibitwarn.py - Warn beta evolve users of the new inhibit extension
#
# Copyright 2015 Facebook, Inc.
#
# As we are rolling out inhibit, our evolve beta testers have to change their
# config to keep using evolve unhinibitted as before. The goal of this extension
# is to warn these users about inhibit and tell them how to deactivate it.
#
# To know who those users are we check the date of oldest obsolescence marker.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import extensions
from mercurial import localrepo
import datetime
msg = """
+------------------------------------------------------------------------------+
|You seem to be an evolve beta user. We installed the inhibit extension        |
|on your computer and it will inhibit the effect of evolve and disturb         |
|your workflow. You need to disable inhibit in your .hgrc to keep working      |
|with evolve. Use hg config --local to open your local config and add the      |
|following lines:                                                              |
|[extensions]                                                                  |
|inhibit=!                                                                     |
|directaccess=!                                                                |
|[experimental]                                                                |
|evolution=all                                                                 |
|                                                                              |
|If you are no longer an evolve beta user and you don't want to see this error |
|with evolve use hg config --local to open your local config and add the next  |
|two lines:                                                                    |
|[inhibit]                                                                     |
|bypass-warning=True                                                           |
|You shouldn't need to do anything else to make inhibit work for this repo.    |
+------------------------------------------------------------------------------+
"""
# Wether the warning message has been displayed already
state = {'displayed': False}

def reposetup(ui, repo):
    # No need to check anything if inhibit is not enabled
    try:
        if not extensions.find('inhibit'):
            return
    except KeyError:
        return

    bypass = repo.ui.configbool('inhibit', 'bypass-warning', False)
    if bypass:
        return
    cutoffdate = repo.ui.config('inhibit', 'cutoff') or '2015-05-18'
    cutofftime = int(datetime.datetime.strptime(cutoffdate,
                    '%Y-%m-%d').strftime("%s"))
    if repo.local():
        for marker in repo.obsstore._all:
            timestamp = marker[4][0]
            if timestamp < cutofftime and not state['displayed']:
                state['displayed'] = True
                ui.write_err(msg)
            # Only the first marker is checked as they are ordered chronologically
            break

