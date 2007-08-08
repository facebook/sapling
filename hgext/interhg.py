# interhg.py - interhg
#
# Copyright 2007 OHASHI Hideya <ohachige@gmail.com>
#
# This software may be used and distributed according to the terms
# of the GNU General Public License, incorporated herein by reference.
#
# The `interhg' Mercurial extension allows you to change changelog and
# summary text just like InterWiki way.
#
# To enable this extension:
#
#   [extensions]
#   interhg =
#
# This is an example to link to a bug tracking system.
#
#   [interhg]
#   pat1 = s/issue(\d+)/ <a href="http:\/\/bts\/issue\1">issue\1<\/a> /
#
# You can add patterns to use pat2, pat3, ...
# For exapmle.
#
#   pat2 = s/(^|\s)#(\d+)\b/ <b>#\2<\/b> /

import re
from mercurial.hgweb import hgweb_mod
from mercurial import templater

orig_escape = templater.common_filters["escape"]

interhg_table = []

def interhg_escape(x):
    escstr = orig_escape(x)
    for pat in interhg_table:
        regexp = pat[0]
        format = pat[1]
        escstr = regexp.sub(format, escstr)
    return escstr

templater.common_filters["escape"] = interhg_escape

orig_refresh = hgweb_mod.hgweb.refresh

def interhg_refresh(self):
    interhg_table[:] = []
    num = 1
    while True:
        key = 'pat%d' % num
        pat = self.config('interhg', key)
        if pat == None:
            break
        pat = pat[2:-1]
        span = re.search(r'[^\\]/', pat).span()
        regexp = pat[:span[0] + 1]
        format = pat[span[1]:]
        format = re.sub(r'\\/', '/', format)
        regexp = re.compile(regexp)
        interhg_table.append((regexp, format))
        num += 1
    return orig_refresh(self)

hgweb_mod.hgweb.refresh = interhg_refresh
