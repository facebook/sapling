# phabdiff.py
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from mercurial import templatekw

import re

def showphabdiff(repo, ctx, templ, **args):
    """:phabdiff: String. Return the phabricator diff id for a given hg rev"""
    descr = ctx.description()
    match = re.search('Differential Revision: https://phabricator.fb.com/(D\d+)', descr)
    return match.group(1) if match else ''

def showtasks(**args):
    """:tasks: String. Return the phabricator diff id for a given hg rev"""
    descr = args['ctx'].description()
    match = re.search('Tasks: (\d+)(,\s*\d+)*', descr)

    if match:
        tasksline = match.group(0)
        tasks = re.findall("\d+", tasksline)
        tasks = ["T%s" % task for task in tasks]
        return templatekw.showlist('task', tasks, **args)
    else:
        return ''

def extsetup(ui):
    templatekw.keywords['phabdiff'] = showphabdiff
    templatekw.keywords['tasks'] = showtasks
