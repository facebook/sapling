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
import datetime
defaultmsg = """
+------------------------------------------------------------------------------+
|You seems to be a beta user of Changeset Evolution                            |
|https://fb.facebook.com/groups/630370820344870/                               |
|                                                                              |
|We just rolled out a major change to our mercurial                            |
|https://fb.facebook.com/groups/scm.fyi/permalink/711128702353004/             |
|                                                                              |
|The rollout contains a lightweight version of Evolution that break your usual |
|workflow using the "hg evolve" commands:                                      |
| https://fb.facebook.com/groups/630370820344870/permalink/907861022595847/    |
|                                                                              |
|If you want to keep using evolve run `hg config -e` and add this to your      |
|config:                                                                       |
|[extensions]                                                                  |
|inhibit=!                                                                     |
|directaccess=!                                                                |
|[experimental]                                                                |
|evolution=all                                                                 |
|                                                                              |
|If you have no recollection of using evolution or stopped using it. run       |
|`hg config -e` and add this to your config:                                   |
|[inhibit]                                                                     |
|bypass-warning=True                                                           |
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
                configmsg = ui.config('inhibitwarn', 'education')
                if configmsg:
                    ui.write_err(configmsg + "\n")
                else:
                    ui.write_err(defaultmsg)
            # Check the first marker as markers are ordered chronologically
            break

