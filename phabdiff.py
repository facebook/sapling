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

def extsetup(ui):
    templatekw.keywords['phabdiff'] = showphabdiff
