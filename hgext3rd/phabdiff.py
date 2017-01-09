# phabdiff.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw
from phabricator import diffprops
import re

taskregex = re.compile('(Tasks|Task ID):\s*(\d+)(,\s*\d+)*')

def showphabdiff(repo, ctx, templ, **args):
    """:phabdiff: String. Return the phabricator diff id for a given hg rev."""
    descr = ctx.description()
    revision = diffprops.parserevfromcommitmsg(descr)
    return 'D' + revision if revision else ''

def showtasks(**args):
    """:tasks: String. Return the tasks associated with given hg rev."""
    descr = args['ctx'].description()
    match = taskregex.search(descr)

    tasks = []
    if match:
        tasksline = match.group(0)
        tasks = re.findall("\d+", tasksline)
    return templatekw.showlist('task', tasks, **args)

def extsetup(ui):
    templatekw.keywords['phabdiff'] = showphabdiff
    templatekw.keywords['tasks'] = showtasks
