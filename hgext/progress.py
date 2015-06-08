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

The following settings are available::

  [progress]
  delay = 3 # number of seconds (float) before showing the progress bar
  changedelay = 1 # changedelay: minimum delay before showing a new topic.
                  # If set to less than 3 * refresh, that value will
                  # be used instead.
  refresh = 0.1 # time in seconds between refreshes of the progress bar
  format = topic bar number estimate # format of the progress bar
  width = <none> # if set, the maximum width of the progress information
                 # (that is, min(width, term width) will be used)
  clear-complete = True # clear the progress bar after it's done
  disable = False # if true, don't show a progress bar
  assume-tty = False # if true, ALWAYS show a progress bar, unless
                     # disable is given

Valid entries for the format field are topic, bar, number, unit,
estimate, speed, and item. item defaults to the last 20 characters of
the item, but this can be changed by adding either ``-<num>`` which
would take the last num characters, or ``+<num>`` for the first num
characters.
"""

def uisetup(ui):
    if ui.config('progress', 'disable', None) is None:
        ui.setconfig('progress', 'disable', 'False', 'hgext-progress')

def reposetup(ui, repo):
    uisetup(repo.ui)
