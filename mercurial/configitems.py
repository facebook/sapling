# configitems.py - centralized declaration of configuration option
#
#  Copyright 2017 Pierre-Yves David <pierre-yves.david@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools

from . import (
    error,
)

def loadconfigtable(ui, extname, configtable):
    """update config item known to the ui with the extension ones"""
    for section, items in configtable.items():
        knownitems = ui._knownconfig.setdefault(section, {})
        knownkeys = set(knownitems)
        newkeys = set(items)
        for key in sorted(knownkeys & newkeys):
            msg = "extension '%s' overwrite config item '%s.%s'"
            msg %= (extname, section, key)
            ui.develwarn(msg, config='warn-config')

        knownitems.update(items)

class configitem(object):
    """represent a known config item

    :section: the official config section where to find this item,
       :name: the official name within the section,
    :default: default value for this item,
    """

    def __init__(self, section, name, default=None):
        self.section = section
        self.name = name
        self.default = default

coreitems = {}

def _register(configtable, *args, **kwargs):
    item = configitem(*args, **kwargs)
    section = configtable.setdefault(item.section, {})
    if item.name in section:
        msg = "duplicated config item registration for '%s.%s'"
        raise error.ProgrammingError(msg % (item.section, item.name))
    section[item.name] = item

# Registering actual config items

def getitemregister(configtable):
    return functools.partial(_register, configtable)

coreconfigitem = getitemregister(coreitems)

coreconfigitem('auth', 'cookiefile',
    default=None,
)
# bookmarks.pushing: internal hack for discovery
coreconfigitem('bookmarks', 'pushing',
    default=list,
)
# bundle.mainreporoot: internal hack for bundlerepo
coreconfigitem('bundle', 'mainreporoot',
    default='',
)
# bundle.reorder: experimental config
coreconfigitem('bundle', 'reorder',
    default='auto',
)
coreconfigitem('color', 'mode',
    default='auto',
)
coreconfigitem('devel', 'all-warnings',
    default=False,
)
coreconfigitem('devel', 'bundle2.debug',
    default=False,
)
coreconfigitem('devel', 'check-locks',
    default=False,
)
coreconfigitem('devel', 'check-relroot',
    default=False,
)
coreconfigitem('devel', 'disableloaddefaultcerts',
    default=False,
)
coreconfigitem('devel', 'legacy.exchange',
    default=list,
)
coreconfigitem('devel', 'servercafile',
    default='',
)
coreconfigitem('devel', 'serverexactprotocol',
    default='',
)
coreconfigitem('devel', 'serverrequirecert',
    default=False,
)
coreconfigitem('devel', 'strip-obsmarkers',
    default=True,
)
coreconfigitem('format', 'aggressivemergedeltas',
    default=False,
)
coreconfigitem('format', 'chunkcachesize',
    default=None,
)
coreconfigitem('format', 'dotencode',
    default=True,
)
coreconfigitem('format', 'generaldelta',
    default=False,
)
coreconfigitem('format', 'manifestcachesize',
    default=None,
)
coreconfigitem('format', 'maxchainlen',
    default=None,
)
coreconfigitem('format', 'obsstore-version',
    default=None,
)
coreconfigitem('format', 'usefncache',
    default=True,
)
coreconfigitem('format', 'usegeneraldelta',
    default=True,
)
coreconfigitem('format', 'usestore',
    default=True,
)
coreconfigitem('hostsecurity', 'ciphers',
    default=None,
)
coreconfigitem('hostsecurity', 'disabletls10warning',
    default=False,
)
coreconfigitem('patch', 'eol',
    default='strict',
)
coreconfigitem('patch', 'fuzz',
    default=2,
)
coreconfigitem('server', 'bundle1',
    default=True,
)
coreconfigitem('server', 'bundle1gd',
    default=None,
)
coreconfigitem('server', 'compressionengines',
    default=list,
)
coreconfigitem('server', 'concurrent-push-mode',
    default='strict',
)
coreconfigitem('server', 'disablefullbundle',
    default=False,
)
coreconfigitem('server', 'maxhttpheaderlen',
    default=1024,
)
coreconfigitem('server', 'preferuncompressed',
    default=False,
)
coreconfigitem('server', 'uncompressedallowsecret',
    default=False,
)
coreconfigitem('server', 'validate',
    default=False,
)
coreconfigitem('server', 'zliblevel',
    default=-1,
)
coreconfigitem('ui', 'clonebundleprefers',
    default=list,
)
coreconfigitem('ui', 'interactive',
    default=None,
)
coreconfigitem('ui', 'quiet',
    default=False,
)
# Windows defaults to a limit of 512 open files. A buffer of 128
# should give us enough headway.
coreconfigitem('worker', 'backgroundclosemaxqueue',
    default=384,
)
coreconfigitem('worker', 'backgroundcloseminfilecount',
    default=2048,
)
coreconfigitem('worker', 'backgroundclosethreadcount',
    default=4,
)
coreconfigitem('worker', 'numcpus',
    default=None,
)
