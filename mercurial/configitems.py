# configitems.py - centralized declaration of configuration option
#
#  Copyright 2017 Pierre-Yves David <pierre-yves.david@octobus.net>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import functools
import re

from . import (
    encoding,
    error,
)

def loadconfigtable(ui, extname, configtable):
    """update config item known to the ui with the extension ones"""
    for section, items in configtable.items():
        knownitems = ui._knownconfig.setdefault(section, itemregister())
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
    :alias: optional list of tuples as alternatives,
    :generic: this is a generic definition, match name using regular expression.
    """

    def __init__(self, section, name, default=None, alias=(),
                 generic=False, priority=0):
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
        if key in self:
            return self[key]

        # search for a matching generic item
        generics = sorted(self._generics, key=(lambda x: (x.priority, x.name)))
        for item in generics:
            if item._re.match(key):
                return item

        # fallback to dict get
        return super(itemregister, self).get(key)

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
    return functools.partial(_register, configtable)

coreconfigitem = getitemregister(coreitems)

coreconfigitem('alias', '.*',
    default=None,
    generic=True,
)
coreconfigitem('annotate', 'nodates',
    default=False,
)
coreconfigitem('annotate', 'showfunc',
    default=False,
)
coreconfigitem('annotate', 'unified',
    default=None,
)
coreconfigitem('annotate', 'git',
    default=False,
)
coreconfigitem('annotate', 'ignorews',
    default=False,
)
coreconfigitem('annotate', 'ignorewsamount',
    default=False,
)
coreconfigitem('annotate', 'ignoreblanklines',
    default=False,
)
coreconfigitem('annotate', 'ignorewseol',
    default=False,
)
coreconfigitem('annotate', 'nobinary',
    default=False,
)
coreconfigitem('annotate', 'noprefix',
    default=False,
)
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
coreconfigitem('color', '.*',
    default=None,
    generic=True,
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
coreconfigitem('commands', 'update.check',
    default=None,
    # Deprecated, remove after 4.4 release
    alias=[('experimental', 'updatecheck')]
)
coreconfigitem('commands', 'update.requiredest',
    default=False,
)
coreconfigitem('committemplate', '.*',
    default=None,
    generic=True,
)
coreconfigitem('debug', 'dirstate.delaywrite',
    default=0,
)
coreconfigitem('defaults', '.*',
    default=None,
    generic=True,
)
coreconfigitem('devel', 'all-warnings',
    default=False,
)
coreconfigitem('devel', 'bundle2.debug',
    default=False,
)
coreconfigitem('devel', 'cache-vfs',
    default=None,
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
coreconfigitem('devel', 'warn-empty-changegroup',
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
coreconfigitem('devel', 'warn-config',
    default=None,
)
coreconfigitem('devel', 'warn-config-default',
    default=None,
)
coreconfigitem('devel', 'user.obsmarker',
    default=None,
)
coreconfigitem('devel', 'warn-config-unknown',
    default=None,
)
coreconfigitem('diff', 'nodates',
    default=False,
)
coreconfigitem('diff', 'showfunc',
    default=False,
)
coreconfigitem('diff', 'unified',
    default=None,
)
coreconfigitem('diff', 'git',
    default=False,
)
coreconfigitem('diff', 'ignorews',
    default=False,
)
coreconfigitem('diff', 'ignorewsamount',
    default=False,
)
coreconfigitem('diff', 'ignoreblanklines',
    default=False,
)
coreconfigitem('diff', 'ignorewseol',
    default=False,
)
coreconfigitem('diff', 'nobinary',
    default=False,
)
coreconfigitem('diff', 'noprefix',
    default=False,
)
coreconfigitem('email', 'bcc',
    default=None,
)
coreconfigitem('email', 'cc',
    default=None,
)
coreconfigitem('email', 'charsets',
    default=list,
)
coreconfigitem('email', 'from',
    default=None,
)
coreconfigitem('email', 'method',
    default='smtp',
)
coreconfigitem('email', 'reply-to',
    default=None,
)
coreconfigitem('experimental', 'allowdivergence',
    default=False,
)
coreconfigitem('experimental', 'archivemetatemplate',
    default=dynamicdefault,
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
coreconfigitem('experimental', 'copytrace',
    default='on',
)
coreconfigitem('experimental', 'copytrace.movecandidateslimit',
    default=100,
)
coreconfigitem('experimental', 'copytrace.sourcecommitlimit',
    default=100,
)
coreconfigitem('experimental', 'crecordtest',
    default=None,
)
coreconfigitem('experimental', 'editortmpinhg',
    default=False,
)
coreconfigitem('experimental', 'evolution',
    default=list,
)
coreconfigitem('experimental', 'evolution.allowunstable',
    default=None,
)
coreconfigitem('experimental', 'evolution.createmarkers',
    default=None,
)
coreconfigitem('experimental', 'evolution.exchange',
    default=None,
)
coreconfigitem('experimental', 'evolution.bundle-obsmarker',
    default=False,
)
coreconfigitem('experimental', 'evolution.track-operation',
    default=True,
)
coreconfigitem('experimental', 'maxdeltachainspan',
    default=-1,
)
coreconfigitem('experimental', 'mmapindexthreshold',
    default=None,
)
coreconfigitem('experimental', 'nonnormalparanoidcheck',
    default=False,
)
coreconfigitem('experimental', 'effect-flags',
    default=False,
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
coreconfigitem('experimental', 'graphstyle.parent',
    default=dynamicdefault,
)
coreconfigitem('experimental', 'graphstyle.missing',
    default=dynamicdefault,
)
coreconfigitem('experimental', 'graphstyle.grandparent',
    default=dynamicdefault,
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
coreconfigitem('experimental', 'rebase.multidest',
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
coreconfigitem('experimental', 'sparse-read',
    default=False,
)
coreconfigitem('experimental', 'sparse-read.density-threshold',
    default=0.25,
)
coreconfigitem('experimental', 'sparse-read.min-block-size',
    default='256K',
)
coreconfigitem('experimental', 'treemanifest',
    default=False,
)
coreconfigitem('extensions', '.*',
    default=None,
    generic=True,
)
coreconfigitem('extdata', '.*',
    default=None,
    generic=True,
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
coreconfigitem('hooks', '.*',
    default=dynamicdefault,
    generic=True,
)
coreconfigitem('hgweb-paths', '.*',
    default=list,
    generic=True,
)
coreconfigitem('hostfingerprints', '.*',
    default=list,
    generic=True,
)
coreconfigitem('hostsecurity', 'ciphers',
    default=None,
)
coreconfigitem('hostsecurity', 'disabletls10warning',
    default=False,
)
coreconfigitem('hostsecurity', 'minimumprotocol',
    default=dynamicdefault,
)
coreconfigitem('hostsecurity', '.*:minimumprotocol$',
    default=dynamicdefault,
    generic=True,
)
coreconfigitem('hostsecurity', '.*:ciphers$',
    default=dynamicdefault,
    generic=True,
)
coreconfigitem('hostsecurity', '.*:fingerprints$',
    default=list,
    generic=True,
)
coreconfigitem('hostsecurity', '.*:verifycertsfile$',
    default=None,
    generic=True,
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
coreconfigitem('logtoprocess', 'commandexception',
    default=None,
)
coreconfigitem('logtoprocess', 'commandfinish',
    default=None,
)
coreconfigitem('logtoprocess', 'command',
    default=None,
)
coreconfigitem('logtoprocess', 'develwarn',
    default=None,
)
coreconfigitem('logtoprocess', 'uiblocked',
    default=None,
)
coreconfigitem('merge', 'checkunknown',
    default='abort',
)
coreconfigitem('merge', 'checkignored',
    default='abort',
)
coreconfigitem('merge', 'followcopies',
    default=True,
)
coreconfigitem('merge', 'on-failure',
    default='continue',
)
coreconfigitem('merge', 'preferancestor',
        default=lambda: ['*'],
)
coreconfigitem('merge-tools', '.*',
    default=None,
    generic=True,
)
coreconfigitem('merge-tools', r'.*\.args$',
    default="$local $base $other",
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.binary$',
    default=False,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.check$',
    default=list,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.checkchanged$',
    default=False,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.executable$',
    default=dynamicdefault,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.fixeol$',
    default=False,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.gui$',
    default=False,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.priority$',
    default=0,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.premerge$',
    default=dynamicdefault,
    generic=True,
    priority=-1,
)
coreconfigitem('merge-tools', r'.*\.symlink$',
    default=False,
    generic=True,
    priority=-1,
)
coreconfigitem('pager', 'attend-.*',
    default=dynamicdefault,
    generic=True,
)
coreconfigitem('pager', 'ignore',
    default=list,
)
coreconfigitem('pager', 'pager',
    default=dynamicdefault,
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
coreconfigitem('paths', '.*',
    default=None,
    generic=True,
)
coreconfigitem('phases', 'checksubrepos',
    default='follow',
)
coreconfigitem('phases', 'new-commit',
    default='draft',
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
coreconfigitem('profiling', 'output',
    default=None,
)
coreconfigitem('profiling', 'showmax',
    default=0.999,
)
coreconfigitem('profiling', 'showmin',
    default=dynamicdefault,
)
coreconfigitem('profiling', 'sort',
    default='inlinetime',
)
coreconfigitem('profiling', 'statformat',
    default='hotpath',
)
coreconfigitem('profiling', 'type',
    default='stat',
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
coreconfigitem('progress', 'estimateinterval',
    default=60.0,
)
coreconfigitem('progress', 'format',
    default=lambda: ['topic', 'bar', 'number', 'estimate'],
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
coreconfigitem('server', 'bundle1.pull',
    default=None,
)
coreconfigitem('server', 'bundle1gd.pull',
    default=None,
)
coreconfigitem('server', 'bundle1.push',
    default=None,
)
coreconfigitem('server', 'bundle1gd.push',
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
coreconfigitem('smtp', 'port',
    default=dynamicdefault,
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
coreconfigitem('templates', '.*',
    default=None,
    generic=True,
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
coreconfigitem('ui', 'interface.chunkselector',
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
coreconfigitem('web', 'allowbz2',
    default=False,
)
coreconfigitem('web', 'allowgz',
    default=False,
)
coreconfigitem('web', 'allowpull',
    default=True,
)
coreconfigitem('web', 'allow_push',
    default=list,
)
coreconfigitem('web', 'allowzip',
    default=False,
)
coreconfigitem('web', 'archivesubrepos',
    default=False,
)
coreconfigitem('web', 'cache',
    default=True,
)
coreconfigitem('web', 'contact',
    default=None,
)
coreconfigitem('web', 'deny_push',
    default=list,
)
coreconfigitem('web', 'guessmime',
    default=False,
)
coreconfigitem('web', 'hidden',
    default=False,
)
coreconfigitem('web', 'labels',
    default=list,
)
coreconfigitem('web', 'logoimg',
    default='hglogo.png',
)
coreconfigitem('web', 'logourl',
    default='https://mercurial-scm.org/',
)
coreconfigitem('web', 'accesslog',
    default='-',
)
coreconfigitem('web', 'address',
    default='',
)
coreconfigitem('web', 'allow_archive',
    default=list,
)
coreconfigitem('web', 'allow_read',
    default=list,
)
coreconfigitem('web', 'baseurl',
    default=None,
)
coreconfigitem('web', 'cacerts',
    default=None,
)
coreconfigitem('web', 'certificate',
    default=None,
)
coreconfigitem('web', 'collapse',
    default=False,
)
coreconfigitem('web', 'csp',
    default=None,
)
coreconfigitem('web', 'deny_read',
    default=list,
)
coreconfigitem('web', 'descend',
    default=True,
)
coreconfigitem('web', 'description',
    default="",
)
coreconfigitem('web', 'encoding',
    default=lambda: encoding.encoding,
)
coreconfigitem('web', 'errorlog',
    default='-',
)
coreconfigitem('web', 'ipv6',
    default=False,
)
coreconfigitem('web', 'maxchanges',
    default=10,
)
coreconfigitem('web', 'maxfiles',
    default=10,
)
coreconfigitem('web', 'maxshortchanges',
    default=60,
)
coreconfigitem('web', 'motd',
    default='',
)
coreconfigitem('web', 'name',
    default=dynamicdefault,
)
coreconfigitem('web', 'port',
    default=8000,
)
coreconfigitem('web', 'prefix',
    default='',
)
coreconfigitem('web', 'push_ssl',
    default=True,
)
coreconfigitem('web', 'refreshinterval',
    default=20,
)
coreconfigitem('web', 'staticurl',
    default=None,
)
coreconfigitem('web', 'stripes',
    default=1,
)
coreconfigitem('web', 'style',
    default='paper',
)
coreconfigitem('web', 'templates',
    default=None,
)
coreconfigitem('web', 'view',
    default='served',
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

# Rebase related configuration moved to core because other extension are doing
# strange things. For example, shelve import the extensions to reuse some bit
# without formally loading it.
coreconfigitem('commands', 'rebase.requiredest',
            default=False,
)
coreconfigitem('experimental', 'rebaseskipobsolete',
    default=True,
)
coreconfigitem('rebase', 'singletransaction',
    default=False,
)
