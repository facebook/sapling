# extorder.py - dependencies for extensions
#
# Copyright 2015 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
"""
Loading order for extensions.

In the extorder section of your hgrc you can define order of extension loading.
For example:
  [extorder]
  extension1 = extension3, extension4
  extension2 = extension1

This will cause the extension1 to be loaded after 3 and 4. Also extension2 will
be loaded after extension1.

Also there are two special configs in this section: 'preferlast' and
'preferfirst'. Those are lists of extensions which prefer to be loaded first or
last. But these are not guaranteed -- normal dependencies have higher priority.

Please not that this extension modifies only order of loading extensions. It
will not load them for you
"""

from mercurial import extensions
from mercurial import error
try:
    from mercurial import chgserver
except ImportError:
    from hgext import chgserver
chgserver._configsections.append('extorder')

testedwith = 'ships-with-fb-hgext'

def uisetup(ui):

    deps = {}
    preferlast = []
    preferfirst = []

    for item, _v in ui.configitems('extorder'):
        val = ui.configlist('extorder', item)
        if item == 'preferlast':
            preferlast.extend(val)
        elif item == 'preferfirst':
            preferfirst.extend(val)
        else:
            deps[item] = val

    exts = list(extensions._order)
    for e in preferfirst + preferlast:
        exts.remove(e)
    unvisited = preferfirst + exts + preferlast

    temp = set()
    order = list()

    def visit(n):
        if n in temp:
            raise error.Abort("extorder: conflicting extension order")
        elif n in unvisited:
            temp.add(n)
            for m in deps.get(n, []):
                visit(m)
            unvisited.remove(n)
            temp.remove(n)
            order.append(n)
    while len(unvisited) > 0:
        visit(unvisited[0])

    extensions._order = order
