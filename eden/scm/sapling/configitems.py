# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# configitems.py - centralized declaration of configuration option
#
#  Copyright 2017 Pierre-Yves David <pierre-yves.david@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.


import functools
import re

from . import encoding, error, util


def loadconfigtable(ui, extname, configtable):
    """update config item known to the ui with the extension ones"""
    for section, items in configtable.items():
        knownitems = ui.uiconfig()._knownconfig.setdefault(section, itemregister())
        knownkeys = set(knownitems)
        newkeys = set(items)
        for key in sorted(knownkeys & newkeys):
            msg = "extension '%s' overwrite config item '%s.%s'"
            msg %= (extname, section, key)
            ui.develwarn(msg, config="warn-config")

        knownitems.update(items)


class configitem:
    """represent a known config item

    :section: the official config section where to find this item,
       :name: the official name within the section,
    :default: default value for this item,
    :alias: optional list of tuples as alternatives,
    :generic: this is a generic definition, match name using regular expression.
    """

    def __init__(
        self, section, name, default=None, alias=(), generic=False, priority=0
    ):
        self.section = section
        self.name = name
        self.default = default
        self.alias = list(alias)
        self.generic = generic
        self.priority = priority
        self._re = None
        if generic:
            self._re = re.compile(self.name)


class itemregister(dict):
    """A specialized dictionary that can handle wild-card selection"""

    def __init__(self):
        super(itemregister, self).__init__()
        self._generics = set()

    def update(self, other):
        super(itemregister, self).update(other)
        self._generics.update(other._generics)

    def __setitem__(self, key, item):
        super(itemregister, self).__setitem__(key, item)
        if item.generic:
            self._generics.add(item)

    def get(self, key):
        baseitem = super(itemregister, self).get(key)
        if baseitem is not None and not baseitem.generic:
            return baseitem

        # search for a matching generic item
        generics = sorted(self._generics, key=(lambda x: (x.priority, x.name)))
        for item in generics:
            # we use 'match' instead of 'search' to make the matching simpler
            # for people unfamiliar with regular expression. Having the match
            # rooted to the start of the string will produce less surprising
            # result for user writing simple regex for sub-attribute.
            #
            # For example using "color\..*" match produces an unsurprising
            # result, while using search could suddenly match apparently
            # unrelated configuration that happens to contains "color."
            # anywhere. This is a tradeoff where we favor requiring ".*" on
            # some match to avoid the need to prefix most pattern with "^".
            # The "^" seems more error prone.
            if item._re.match(key):
                return item

        return None


coreitems = {}


def _register(configtable, *args, **kwargs):
    item = configitem(*args, **kwargs)
    section = configtable.setdefault(item.section, itemregister())
    if item.name in section:
        msg = "duplicated config item registration for '%s.%s'"
        raise error.ProgrammingError(msg % (item.section, item.name))
    section[item.name] = item


# special value for case where the default is derived from other values
dynamicdefault = object()

# Registering actual config items


def getitemregister(configtable):
    f = functools.partial(_register, configtable)
    # export pseudo enum as configitem.*
    f.dynamicdefault = dynamicdefault
    return f
