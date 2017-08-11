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
    :alias: optional list of tuples as alternatives.
    """

    def __init__(self, section, name, default=None, alias=()):
        self.section = section
        self.name = name
        self.default = default
        self.alias = list(alias)

coreitems = {}

def _register(configtable, *args, **kwargs):
    item = configitem(*args, **kwargs)
    section = configtable.setdefault(item.section, {})
    if item.name in section:
        msg = "duplicated config item registration for '%s.%s'"
        raise error.ProgrammingError(msg % (item.section, item.name))
    section[item.name] = item

# special value for case where the default is derived from other values
dynamicdefault = object()

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
coreconfigitem('censor', 'policy',
    default='abort',
)
coreconfigitem('chgserver', 'idletimeout',
    default=3600,
)
coreconfigitem('chgserver', 'skiphash',
    default=False,
)
coreconfigitem('cmdserver', 'log',
    default=None,
)
coreconfigitem('color', 'mode',
    default='auto',
)
coreconfigitem('color', 'pagermode',
    default=dynamicdefault,
)
coreconfigitem('commands', 'status.relative',
    default=False,
)
coreconfigitem('commands', 'status.skipstates',
    default=[],
)
coreconfigitem('commands', 'status.verbose',
    default=False,
)
coreconfigitem('commands', 'update.requiredest',
    default=False,
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
coreconfigitem('devel', 'default-date',
    default=None,
)
coreconfigitem('devel', 'deprec-warn',
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
coreconfigitem('email', 'charsets',
    default=list,
)
coreconfigitem('email', 'method',
    default='smtp',
)
coreconfigitem('experimental', 'bundle-phases',
    default=False,
)
coreconfigitem('experimental', 'bundle2-advertise',
    default=True,
)
coreconfigitem('experimental', 'bundle2-output-capture',
    default=False,
)
coreconfigitem('experimental', 'bundle2.pushback',
    default=False,
)
coreconfigitem('experimental', 'bundle2lazylocking',
    default=False,
)
coreconfigitem('experimental', 'bundlecomplevel',
    default=None,
)
coreconfigitem('experimental', 'changegroup3',
    default=False,
)
coreconfigitem('experimental', 'clientcompressionengines',
    default=list,
)
coreconfigitem('experimental', 'crecordtest',
    default=None,
)
coreconfigitem('experimental', 'disablecopytrace',
    default=False,
)
coreconfigitem('experimental', 'editortmpinhg',
    default=False,
)
coreconfigitem('experimental', 'stabilization',
    default=list,
    alias=[('experimental', 'evolution')],
)
coreconfigitem('experimental', 'stabilization.bundle-obsmarker',
    default=False,
    alias=[('experimental', 'evolution.bundle-obsmarker')],
)
coreconfigitem('experimental', 'stabilization.track-operation',
    default=False,
    alias=[('experimental', 'evolution.track-operation')]
)
coreconfigitem('experimental', 'exportableenviron',
    default=list,
)
coreconfigitem('experimental', 'extendedheader.index',
    default=None,
)
coreconfigitem('experimental', 'extendedheader.similarity',
    default=False,
)
coreconfigitem('experimental', 'format.compression',
    default='zlib',
)
coreconfigitem('experimental', 'graphshorten',
    default=False,
)
coreconfigitem('experimental', 'hook-track-tags',
    default=False,
)
coreconfigitem('experimental', 'httppostargs',
    default=False,
)
coreconfigitem('experimental', 'manifestv2',
    default=False,
)
coreconfigitem('experimental', 'mergedriver',
    default=None,
)
coreconfigitem('experimental', 'obsmarkers-exchange-debug',
    default=False,
)
coreconfigitem('experimental', 'revertalternateinteractivemode',
    default=True,
)
coreconfigitem('experimental', 'revlogv2',
    default=None,
)
coreconfigitem('experimental', 'spacemovesdown',
    default=False,
)
coreconfigitem('experimental', 'treemanifest',
    default=False,
)
coreconfigitem('experimental', 'updatecheck',
    default=None,
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
coreconfigitem('http_proxy', 'always',
    default=False,
)
coreconfigitem('http_proxy', 'host',
    default=None,
)
coreconfigitem('http_proxy', 'no',
    default=list,
)
coreconfigitem('http_proxy', 'passwd',
    default=None,
)
coreconfigitem('http_proxy', 'user',
    default=None,
)
coreconfigitem('merge', 'followcopies',
    default=True,
)
coreconfigitem('pager', 'ignore',
    default=list,
)
coreconfigitem('patch', 'eol',
    default='strict',
)
coreconfigitem('patch', 'fuzz',
    default=2,
)
coreconfigitem('paths', 'default',
    default=None,
)
coreconfigitem('paths', 'default-push',
    default=None,
)
coreconfigitem('phases', 'checksubrepos',
    default='follow',
)
coreconfigitem('phases', 'publish',
    default=True,
)
coreconfigitem('profiling', 'enabled',
    default=False,
)
coreconfigitem('profiling', 'format',
    default='text',
)
coreconfigitem('profiling', 'freq',
    default=1000,
)
coreconfigitem('profiling', 'limit',
    default=30,
)
coreconfigitem('profiling', 'nested',
    default=0,
)
coreconfigitem('profiling', 'sort',
    default='inlinetime',
)
coreconfigitem('profiling', 'statformat',
    default='hotpath',
)
coreconfigitem('progress', 'assume-tty',
    default=False,
)
coreconfigitem('progress', 'changedelay',
    default=1,
)
coreconfigitem('progress', 'clear-complete',
    default=True,
)
coreconfigitem('progress', 'debug',
    default=False,
)
coreconfigitem('progress', 'delay',
    default=3,
)
coreconfigitem('progress', 'disable',
    default=False,
)
coreconfigitem('progress', 'estimate',
    default=2,
)
coreconfigitem('progress', 'refresh',
    default=0.1,
)
coreconfigitem('progress', 'width',
    default=dynamicdefault,
)
coreconfigitem('push', 'pushvars.server',
    default=False,
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
coreconfigitem('server', 'uncompressed',
    default=True,
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
coreconfigitem('smtp', 'host',
    default=None,
)
coreconfigitem('smtp', 'local_hostname',
    default=None,
)
coreconfigitem('smtp', 'password',
    default=None,
)
coreconfigitem('smtp', 'tls',
    default='none',
)
coreconfigitem('smtp', 'username',
    default=None,
)
coreconfigitem('sparse', 'missingwarning',
    default=True,
)
coreconfigitem('trusted', 'groups',
    default=list,
)
coreconfigitem('trusted', 'users',
    default=list,
)
coreconfigitem('ui', '_usedassubrepo',
    default=False,
)
coreconfigitem('ui', 'allowemptycommit',
    default=False,
)
coreconfigitem('ui', 'archivemeta',
    default=True,
)
coreconfigitem('ui', 'askusername',
    default=False,
)
coreconfigitem('ui', 'clonebundlefallback',
    default=False,
)
coreconfigitem('ui', 'clonebundleprefers',
    default=list,
)
coreconfigitem('ui', 'clonebundles',
    default=True,
)
coreconfigitem('ui', 'color',
    default='auto',
)
coreconfigitem('ui', 'commitsubrepos',
    default=False,
)
coreconfigitem('ui', 'debug',
    default=False,
)
coreconfigitem('ui', 'debugger',
    default=None,
)
coreconfigitem('ui', 'fallbackencoding',
    default=None,
)
coreconfigitem('ui', 'forcecwd',
    default=None,
)
coreconfigitem('ui', 'forcemerge',
    default=None,
)
coreconfigitem('ui', 'formatdebug',
    default=False,
)
coreconfigitem('ui', 'formatjson',
    default=False,
)
coreconfigitem('ui', 'formatted',
    default=None,
)
coreconfigitem('ui', 'graphnodetemplate',
    default=None,
)
coreconfigitem('ui', 'http2debuglevel',
    default=None,
)
coreconfigitem('ui', 'interactive',
    default=None,
)
coreconfigitem('ui', 'interface',
    default=None,
)
coreconfigitem('ui', 'logblockedtimes',
    default=False,
)
coreconfigitem('ui', 'logtemplate',
    default=None,
)
coreconfigitem('ui', 'merge',
    default=None,
)
coreconfigitem('ui', 'mergemarkers',
    default='basic',
)
coreconfigitem('ui', 'mergemarkertemplate',
    default=('{node|short} '
            '{ifeq(tags, "tip", "", '
            'ifeq(tags, "", "", "{tags} "))}'
            '{if(bookmarks, "{bookmarks} ")}'
            '{ifeq(branch, "default", "", "{branch} ")}'
            '- {author|user}: {desc|firstline}')
)
coreconfigitem('ui', 'nontty',
    default=False,
)
coreconfigitem('ui', 'origbackuppath',
    default=None,
)
coreconfigitem('ui', 'paginate',
    default=True,
)
coreconfigitem('ui', 'patch',
    default=None,
)
coreconfigitem('ui', 'portablefilenames',
    default='warn',
)
coreconfigitem('ui', 'promptecho',
    default=False,
)
coreconfigitem('ui', 'quiet',
    default=False,
)
coreconfigitem('ui', 'quietbookmarkmove',
    default=False,
)
coreconfigitem('ui', 'remotecmd',
    default='hg',
)
coreconfigitem('ui', 'report_untrusted',
    default=True,
)
coreconfigitem('ui', 'rollback',
    default=True,
)
coreconfigitem('ui', 'slash',
    default=False,
)
coreconfigitem('ui', 'ssh',
    default='ssh',
)
coreconfigitem('ui', 'statuscopies',
    default=False,
)
coreconfigitem('ui', 'strict',
    default=False,
)
coreconfigitem('ui', 'style',
    default='',
)
coreconfigitem('ui', 'supportcontact',
    default=None,
)
coreconfigitem('ui', 'textwidth',
    default=78,
)
coreconfigitem('ui', 'timeout',
    default='600',
)
coreconfigitem('ui', 'traceback',
    default=False,
)
coreconfigitem('ui', 'tweakdefaults',
    default=False,
)
coreconfigitem('ui', 'usehttp2',
    default=False,
)
coreconfigitem('ui', 'username',
    alias=[('ui', 'user')]
)
coreconfigitem('ui', 'verbose',
    default=False,
)
coreconfigitem('verify', 'skipflags',
    default=None,
)
coreconfigitem('worker', 'backgroundclose',
    default=dynamicdefault,
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
