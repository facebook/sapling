# progress.py show progress bars for some actions
#
# Copyright (C) 2010 Augie Fackler <durin42@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""show progress bars for some actions

This extension uses the progress information logged by hg commands
to draw progress bars that are as informative as possible. Some progress
bars only offer indeterminate information, while others have a definite
end point.
"""

def uisetup(ui):
    if ui.config('progress', 'disable', None) is None:
        ui.setconfig('progress', 'disable', 'False', 'hgext-progress')

def reposetup(ui, repo):
    uisetup(repo.ui)
