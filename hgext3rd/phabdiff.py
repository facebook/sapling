# phabdiff.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw
from phabricator import diffprops
import re

def showphabdiff(repo, ctx, templ, **args):
    """:phabdiff: String. Return the phabricator diff id for a given hg rev."""
    descr = ctx.description()
    revision = diffprops.parserevfromcommitmsg(descr)
    return 'D' + revision if revision else ''

def showtasks(**args):
    """:tasks: String. Return the tasks associated with given hg rev."""
    tasks = []
    descr = args['ctx'].description()
    match = re.search('(Tasks?|Task ID):(.*)', descr)
    if match:
        tasks = re.findall('\d+', match.group(0))
    return templatekw.showlist('task', tasks, **args)

def extsetup(ui):
    templatekw.keywords['phabdiff'] = showphabdiff
    templatekw.keywords['tasks'] = showtasks
