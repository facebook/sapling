# ui.py - user interface bits for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import contextlib
import errno
import getpass
import inspect
import os
import re
import signal
import socket
import subprocess
import sys
import tempfile
import traceback

from .i18n import _
from .node import hex

from . import (
    color,
    config,
    configitems,
    encoding,
    error,
    formatter,
    progress,
    pycompat,
    rcutil,
    scmutil,
    util,
)

urlreq = util.urlreq

# for use with str.translate(None, _keepalnum), to keep just alphanumerics
_keepalnum = ''.join(c for c in map(pycompat.bytechr, range(256))
                     if not c.isalnum())

# The config knobs that will be altered (if unset) by ui.tweakdefaults.
tweakrc = """
[ui]
# The rollback command is dangerous. As a rule, don't use it.
rollback = False
# Make `hg status` report copy information
statuscopies = yes
# Prefer curses UIs when available. Revert to plain-text with `text`.
interface = curses

[commands]
# Make `hg status` emit cwd-relative paths by default.
status.relative = yes
# Refuse to perform an `hg update` that would cause a file content merge
update.check = noconflict

[diff]
git = 1
showfunc = 1
"""

samplehgrcs = {
    'user':
b"""# example user config (see 'hg help config' for more info)
[ui]
# name and email, e.g.
# username = Jane Doe <jdoe@example.com>
username =

# We recommend enabling tweakdefaults to get slight improvements to
# the UI over time. Make sure to set HGPLAIN in the environment when
# writing scripts!
# tweakdefaults = True

# uncomment to disable color in command output
# (see 'hg help color' for details)
# color = never

# uncomment to disable command output pagination
# (see 'hg help pager' for details)
# paginate = never

[extensions]
# uncomment these lines to enable some popular extensions
# (see 'hg help extensions' for more info)
#
# churn =
""",

    'cloned':
b"""# example repository config (see 'hg help config' for more info)
[paths]
default = %s

# path aliases to other clones of this repo in URLs or filesystem paths
# (see 'hg help config.paths' for more info)
#
# default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
# my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
# my-clone        = /home/jdoe/jdoes-clone

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",

    'local':
b"""# example repository config (see 'hg help config' for more info)
[paths]
# path aliases to other clones of this repo in URLs or filesystem paths
# (see 'hg help config.paths' for more info)
#
# default         = http://example.com/hg/example-repo
# default:pushurl = ssh://jdoe@example.net/hg/jdoes-fork
# my-fork         = ssh://jdoe@example.net/hg/jdoes-fork
# my-clone        = /home/jdoe/jdoes-clone

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",

    'global':
b"""# example system-wide hg config (see 'hg help config' for more info)

[ui]
# uncomment to disable color in command output
# (see 'hg help color' for details)
# color = never

# uncomment to disable command output pagination
# (see 'hg help pager' for details)
# paginate = never

[extensions]
# uncomment these lines to enable some popular extensions
# (see 'hg help extensions' for more info)
#
# blackbox =
# churn =
""",
}

def _maybestrurl(maybebytes):
    if maybebytes is None:
        return None
    return pycompat.strurl(maybebytes)

def _maybebytesurl(maybestr):
    if maybestr is None:
        return None
    return pycompat.bytesurl(maybestr)

class httppasswordmgrdbproxy(object):
    """Delays loading urllib2 until it's needed."""
    def __init__(self):
        self._mgr = None

    def _get_mgr(self):
        if self._mgr is None:
            self._mgr = urlreq.httppasswordmgrwithdefaultrealm()
        return self._mgr

    def add_password(self, realm, uris, user, passwd):
        if isinstance(uris, tuple):
            uris = tuple(_maybestrurl(u) for u in uris)
        else:
            uris = _maybestrurl(uris)
        return self._get_mgr().add_password(
            _maybestrurl(realm), uris,
            _maybestrurl(user), _maybestrurl(passwd))

    def find_user_password(self, realm, uri):
        return tuple(_maybebytesurl(v) for v in
                     self._get_mgr().find_user_password(_maybestrurl(realm),
                                                        _maybestrurl(uri)))

def _catchterm(*args):
    raise error.SignalInterrupt

# unique object used to detect no default value has been provided when
# retrieving configuration value.
_unset = object()

# _reqexithandlers: callbacks run at the end of a request
_reqexithandlers = []

class ui(object):
    def __init__(self, src=None):
        """Create a fresh new ui object if no src given

        Use uimod.ui.load() to create a ui which knows global and user configs.
        In most cases, you should use ui.copy() to create a copy of an existing
        ui object.
        """
        # _buffers: used for temporary capture of output
        self._buffers = []
        # 3-tuple describing how each buffer in the stack behaves.
        # Values are (capture stderr, capture subprocesses, apply labels).
        self._bufferstates = []
        # When a buffer is active, defines whether we are expanding labels.
        # This exists to prevent an extra list lookup.
        self._bufferapplylabels = None
        self.quiet = self.verbose = self.debugflag = self.tracebackflag = False
        self._reportuntrusted = True
        self._knownconfig = configitems.coreitems
        self._ocfg = config.config() # overlay
        self._tcfg = config.config() # trusted
        self._ucfg = config.config() # untrusted
        self._trustusers = set()
        self._trustgroups = set()
        self.callhooks = True
        # Insecure server connections requested.
        self.insecureconnections = False
        # Blocked time
        self.logblockedtimes = False
        # color mode: see mercurial/color.py for possible value
        self._colormode = None
        self._terminfoparams = {}
        self._styles = {}

        if src:
            self.fout = src.fout
            self.ferr = src.ferr
            self.fin = src.fin
            self.pageractive = src.pageractive
            self._disablepager = src._disablepager
            self._tweaked = src._tweaked

            self._tcfg = src._tcfg.copy()
            self._ucfg = src._ucfg.copy()
            self._ocfg = src._ocfg.copy()
            self._trustusers = src._trustusers.copy()
            self._trustgroups = src._trustgroups.copy()
            self.environ = src.environ
            self.callhooks = src.callhooks
            self.insecureconnections = src.insecureconnections
            self._colormode = src._colormode
            self._terminfoparams = src._terminfoparams.copy()
            self._styles = src._styles.copy()

            self.fixconfig()

            self.httppasswordmgrdb = src.httppasswordmgrdb
            self._blockedtimes = src._blockedtimes
        else:
            self.fout = util.stdout
            self.ferr = util.stderr
            self.fin = util.stdin
            self.pageractive = False
            self._disablepager = False
            self._tweaked = False

            # shared read-only environment
            self.environ = encoding.environ

            self.httppasswordmgrdb = httppasswordmgrdbproxy()
            self._blockedtimes = collections.defaultdict(int)

        allowed = self.configlist('experimental', 'exportableenviron')
        if '*' in allowed:
            self._exportableenviron = self.environ
        else:
            self._exportableenviron = {}
            for k in allowed:
                if k in self.environ:
                    self._exportableenviron[k] = self.environ[k]

    @classmethod
    def load(cls):
        """Create a ui and load global and user configs"""
        u = cls()
        # we always trust global config files and environment variables
        for t, f in rcutil.rccomponents():
            if t == 'path':
                u.readconfig(f, trust=True)
            elif t == 'items':
                sections = set()
                for section, name, value, source in f:
                    # do not set u._ocfg
                    # XXX clean this up once immutable config object is a thing
                    u._tcfg.set(section, name, value, source)
                    u._ucfg.set(section, name, value, source)
                    sections.add(section)
                for section in sections:
                    u.fixconfig(section=section)
            else:
                raise error.ProgrammingError('unknown rctype: %s' % t)
        u._maybetweakdefaults()
        return u

    def _maybetweakdefaults(self):
        if not self.configbool('ui', 'tweakdefaults'):
            return
        if self._tweaked or self.plain('tweakdefaults'):
            return

        # Note: it is SUPER IMPORTANT that you set self._tweaked to
        # True *before* any calls to setconfig(), otherwise you'll get
        # infinite recursion between setconfig and this method.
        #
        # TODO: We should extract an inner method in setconfig() to
        # avoid this weirdness.
        self._tweaked = True
        tmpcfg = config.config()
        tmpcfg.parse('<tweakdefaults>', tweakrc)
        for section in tmpcfg:
            for name, value in tmpcfg.items(section):
                if not self.hasconfig(section, name):
                    self.setconfig(section, name, value, "<tweakdefaults>")

    def copy(self):
        return self.__class__(self)

    def resetstate(self):
        """Clear internal state that shouldn't persist across commands"""
        if self._progbar:
            self._progbar.resetstate()  # reset last-print time of progress bar
        self.httppasswordmgrdb = httppasswordmgrdbproxy()

    @contextlib.contextmanager
    def timeblockedsection(self, key):
        # this is open-coded below - search for timeblockedsection to find them
        starttime = util.timer()
        try:
            yield
        finally:
            self._blockedtimes[key + '_blocked'] += \
                (util.timer() - starttime) * 1000

    def formatter(self, topic, opts):
        return formatter.formatter(self, self, topic, opts)

    def _trusted(self, fp, f):
        st = util.fstat(fp)
        if util.isowner(st):
            return True

        tusers, tgroups = self._trustusers, self._trustgroups
        if '*' in tusers or '*' in tgroups:
            return True

        user = util.username(st.st_uid)
        group = util.groupname(st.st_gid)
        if user in tusers or group in tgroups or user == util.username():
            return True

        if self._reportuntrusted:
            self.warn(_('not trusting file %s from untrusted '
                        'user %s, group %s\n') % (f, user, group))
        return False

    def readconfig(self, filename, root=None, trust=False,
                   sections=None, remap=None):
        try:
            fp = open(filename, u'rb')
        except IOError:
            if not sections: # ignore unless we were looking for something
                return
            raise

        cfg = config.config()
        trusted = sections or trust or self._trusted(fp, filename)

        try:
            cfg.read(filename, fp, sections=sections, remap=remap)
            fp.close()
        except error.ConfigError as inst:
            if trusted:
                raise
            self.warn(_("ignored: %s\n") % str(inst))

        if self.plain():
            for k in ('debug', 'fallbackencoding', 'quiet', 'slash',
                      'logtemplate', 'statuscopies', 'style',
                      'traceback', 'verbose'):
                if k in cfg['ui']:
                    del cfg['ui'][k]
            for k, v in cfg.items('defaults'):
                del cfg['defaults'][k]
            for k, v in cfg.items('commands'):
                del cfg['commands'][k]
        # Don't remove aliases from the configuration if in the exceptionlist
        if self.plain('alias'):
            for k, v in cfg.items('alias'):
                del cfg['alias'][k]
        if self.plain('revsetalias'):
            for k, v in cfg.items('revsetalias'):
                del cfg['revsetalias'][k]
        if self.plain('templatealias'):
            for k, v in cfg.items('templatealias'):
                del cfg['templatealias'][k]

        if trusted:
            self._tcfg.update(cfg)
            self._tcfg.update(self._ocfg)
        self._ucfg.update(cfg)
        self._ucfg.update(self._ocfg)

        if root is None:
            root = os.path.expanduser('~')
        self.fixconfig(root=root)

    def fixconfig(self, root=None, section=None):
        if section in (None, 'paths'):
            # expand vars and ~
            # translate paths relative to root (or home) into absolute paths
            root = root or pycompat.getcwd()
            for c in self._tcfg, self._ucfg, self._ocfg:
                for n, p in c.items('paths'):
                    # Ignore sub-options.
                    if ':' in n:
                        continue
                    if not p:
                        continue
                    if '%%' in p:
                        s = self.configsource('paths', n) or 'none'
                        self.warn(_("(deprecated '%%' in path %s=%s from %s)\n")
                                  % (n, p, s))
                        p = p.replace('%%', '%')
                    p = util.expandpath(p)
                    if not util.hasscheme(p) and not os.path.isabs(p):
                        p = os.path.normpath(os.path.join(root, p))
                    c.set("paths", n, p)

        if section in (None, 'ui'):
            # update ui options
            self.debugflag = self.configbool('ui', 'debug')
            self.verbose = self.debugflag or self.configbool('ui', 'verbose')
            self.quiet = not self.debugflag and self.configbool('ui', 'quiet')
            if self.verbose and self.quiet:
                self.quiet = self.verbose = False
            self._reportuntrusted = self.debugflag or self.configbool("ui",
                "report_untrusted")
            self.tracebackflag = self.configbool('ui', 'traceback')
            self.logblockedtimes = self.configbool('ui', 'logblockedtimes')

        if section in (None, 'trusted'):
            # update trust information
            self._trustusers.update(self.configlist('trusted', 'users'))
            self._trustgroups.update(self.configlist('trusted', 'groups'))

    def backupconfig(self, section, item):
        return (self._ocfg.backup(section, item),
                self._tcfg.backup(section, item),
                self._ucfg.backup(section, item),)
    def restoreconfig(self, data):
        self._ocfg.restore(data[0])
        self._tcfg.restore(data[1])
        self._ucfg.restore(data[2])

    def setconfig(self, section, name, value, source=''):
        for cfg in (self._ocfg, self._tcfg, self._ucfg):
            cfg.set(section, name, value, source)
        self.fixconfig(section=section)
        self._maybetweakdefaults()

    def _data(self, untrusted):
        return untrusted and self._ucfg or self._tcfg

    def configsource(self, section, name, untrusted=False):
        return self._data(untrusted).source(section, name)

    def config(self, section, name, default=_unset, untrusted=False):
        """return the plain string version of a config"""
        value = self._config(section, name, default=default,
                             untrusted=untrusted)
        if value is _unset:
            return None
        return value

    def _config(self, section, name, default=_unset, untrusted=False):
        value = itemdefault = default
        item = self._knownconfig.get(section, {}).get(name)
        alternates = [(section, name)]

        if item is not None:
            alternates.extend(item.alias)
            if callable(item.default):
                itemdefault = item.default()
            else:
                itemdefault = item.default
        else:
            msg = ("accessing unregistered config item: '%s.%s'")
            msg %= (section, name)
            self.develwarn(msg, 2, 'warn-config-unknown')

        if default is _unset:
            if item is None:
                value = default
            elif item.default is configitems.dynamicdefault:
                value = None
                msg = "config item requires an explicit default value: '%s.%s'"
                msg %= (section, name)
                self.develwarn(msg, 2, 'warn-config-default')
            else:
                value = itemdefault
        elif (item is not None
              and item.default is not configitems.dynamicdefault
              and default != itemdefault):
            msg = ("specifying a mismatched default value for a registered "
                   "config item: '%s.%s' '%s'")
            msg %= (section, name, default)
            self.develwarn(msg, 2, 'warn-config-default')

        for s, n in alternates:
            candidate = self._data(untrusted).get(s, n, None)
            if candidate is not None:
                value = candidate
                section = s
                name = n
                break

        if self.debugflag and not untrusted and self._reportuntrusted:
            for s, n in alternates:
                uvalue = self._ucfg.get(s, n)
                if uvalue is not None and uvalue != value:
                    self.debug("ignoring untrusted configuration option "
                               "%s.%s = %s\n" % (s, n, uvalue))
        return value

    def configsuboptions(self, section, name, default=_unset, untrusted=False):
        """Get a config option and all sub-options.

        Some config options have sub-options that are declared with the
        format "key:opt = value". This method is used to return the main
        option and all its declared sub-options.

        Returns a 2-tuple of ``(option, sub-options)``, where `sub-options``
        is a dict of defined sub-options where keys and values are strings.
        """
        main = self.config(section, name, default, untrusted=untrusted)
        data = self._data(untrusted)
        sub = {}
        prefix = '%s:' % name
        for k, v in data.items(section):
            if k.startswith(prefix):
                sub[k[len(prefix):]] = v

        if self.debugflag and not untrusted and self._reportuntrusted:
            for k, v in sub.items():
                uvalue = self._ucfg.get(section, '%s:%s' % (name, k))
                if uvalue is not None and uvalue != v:
                    self.debug('ignoring untrusted configuration option '
                               '%s:%s.%s = %s\n' % (section, name, k, uvalue))

        return main, sub

    def configpath(self, section, name, default=_unset, untrusted=False):
        'get a path config item, expanded relative to repo root or config file'
        v = self.config(section, name, default, untrusted)
        if v is None:
            return None
        if not os.path.isabs(v) or "://" not in v:
            src = self.configsource(section, name, untrusted)
            if ':' in src:
                base = os.path.dirname(src.rsplit(':')[0])
                v = os.path.join(base, os.path.expanduser(v))
        return v

    def configbool(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as a boolean

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'true', b'yes')
        >>> u.configbool(s, b'true')
        True
        >>> u.setconfig(s, b'false', b'no')
        >>> u.configbool(s, b'false')
        False
        >>> u.configbool(s, b'unknown')
        False
        >>> u.configbool(s, b'unknown', True)
        True
        >>> u.setconfig(s, b'invalid', b'somevalue')
        >>> u.configbool(s, b'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a boolean ('somevalue')
        """

        v = self._config(section, name, default, untrusted=untrusted)
        if v is None:
            return v
        if v is _unset:
            if default is _unset:
                return False
            return default
        if isinstance(v, bool):
            return v
        b = util.parsebool(v)
        if b is None:
            raise error.ConfigError(_("%s.%s is not a boolean ('%s')")
                                    % (section, name, v))
        return b

    def configwith(self, convert, section, name, default=_unset,
                   desc=None, untrusted=False):
        """parse a configuration element with a conversion function

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'float1', b'42')
        >>> u.configwith(float, s, b'float1')
        42.0
        >>> u.setconfig(s, b'float2', b'-4.25')
        >>> u.configwith(float, s, b'float2')
        -4.25
        >>> u.configwith(float, s, b'unknown', 7)
        7.0
        >>> u.setconfig(s, b'invalid', b'somevalue')
        >>> u.configwith(float, s, b'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid float ('somevalue')
        >>> u.configwith(float, s, b'invalid', desc=b'womble')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid womble ('somevalue')
        """

        v = self.config(section, name, default, untrusted)
        if v is None:
            return v # do not attempt to convert None
        try:
            return convert(v)
        except (ValueError, error.ParseError):
            if desc is None:
                desc = pycompat.sysbytes(convert.__name__)
            raise error.ConfigError(_("%s.%s is not a valid %s ('%s')")
                                    % (section, name, desc, v))

    def configint(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as an integer

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'int1', b'42')
        >>> u.configint(s, b'int1')
        42
        >>> u.setconfig(s, b'int2', b'-42')
        >>> u.configint(s, b'int2')
        -42
        >>> u.configint(s, b'unknown', 7)
        7
        >>> u.setconfig(s, b'invalid', b'somevalue')
        >>> u.configint(s, b'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid integer ('somevalue')
        """

        return self.configwith(int, section, name, default, 'integer',
                               untrusted)

    def configbytes(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as a quantity in bytes

        Units can be specified as b (bytes), k or kb (kilobytes), m or
        mb (megabytes), g or gb (gigabytes).

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'val1', b'42')
        >>> u.configbytes(s, b'val1')
        42
        >>> u.setconfig(s, b'val2', b'42.5 kb')
        >>> u.configbytes(s, b'val2')
        43520
        >>> u.configbytes(s, b'unknown', b'7 MB')
        7340032
        >>> u.setconfig(s, b'invalid', b'somevalue')
        >>> u.configbytes(s, b'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a byte quantity ('somevalue')
        """

        value = self._config(section, name, default, untrusted)
        if value is _unset:
            if default is _unset:
                default = 0
            value = default
        if not isinstance(value, bytes):
            return value
        try:
            return util.sizetoint(value)
        except error.ParseError:
            raise error.ConfigError(_("%s.%s is not a byte quantity ('%s')")
                                    % (section, name, value))

    def configlist(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as a list of comma/space separated
        strings

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'list1', b'this,is "a small" ,test')
        >>> u.configlist(s, b'list1')
        ['this', 'is', 'a small', 'test']
        >>> u.setconfig(s, b'list2', b'this, is "a small" , test ')
        >>> u.configlist(s, b'list2')
        ['this', 'is', 'a small', 'test']
        """
        # default is not always a list
        v = self.configwith(config.parselist, section, name, default,
                               'list', untrusted)
        if isinstance(v, bytes):
            return config.parselist(v)
        elif v is None:
            return []
        return v

    def configdate(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as a tuple of ints

        >>> u = ui(); s = b'foo'
        >>> u.setconfig(s, b'date', b'0 0')
        >>> u.configdate(s, b'date')
        (0, 0)
        """
        if self.config(section, name, default, untrusted):
            return self.configwith(util.parsedate, section, name, default,
                                   'date', untrusted)
        if default is _unset:
            return None
        return default

    def hasconfig(self, section, name, untrusted=False):
        return self._data(untrusted).hasitem(section, name)

    def has_section(self, section, untrusted=False):
        '''tell whether section exists in config.'''
        return section in self._data(untrusted)

    def configitems(self, section, untrusted=False, ignoresub=False):
        items = self._data(untrusted).items(section)
        if ignoresub:
            newitems = {}
            for k, v in items:
                if ':' not in k:
                    newitems[k] = v
            items = newitems.items()
        if self.debugflag and not untrusted and self._reportuntrusted:
            for k, v in self._ucfg.items(section):
                if self._tcfg.get(section, k) != v:
                    self.debug("ignoring untrusted configuration option "
                               "%s.%s = %s\n" % (section, k, v))
        return items

    def walkconfig(self, untrusted=False):
        cfg = self._data(untrusted)
        for section in cfg.sections():
            for name, value in self.configitems(section, untrusted):
                yield section, name, value

    def plain(self, feature=None):
        '''is plain mode active?

        Plain mode means that all configuration variables which affect
        the behavior and output of Mercurial should be
        ignored. Additionally, the output should be stable,
        reproducible and suitable for use in scripts or applications.

        The only way to trigger plain mode is by setting either the
        `HGPLAIN' or `HGPLAINEXCEPT' environment variables.

        The return value can either be
        - False if HGPLAIN is not set, or feature is in HGPLAINEXCEPT
        - False if feature is disabled by default and not included in HGPLAIN
        - True otherwise
        '''
        if ('HGPLAIN' not in encoding.environ and
                'HGPLAINEXCEPT' not in encoding.environ):
            return False
        exceptions = encoding.environ.get('HGPLAINEXCEPT',
                '').strip().split(',')
        # TODO: add support for HGPLAIN=+feature,-feature syntax
        if '+strictflags' not in encoding.environ.get('HGPLAIN', '').split(','):
            exceptions.append('strictflags')
        if feature and exceptions:
            return feature not in exceptions
        return True

    def username(self, acceptempty=False):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        If not found and acceptempty is True, returns None.
        If not found and ui.askusername is True, ask the user, else use
        ($LOGNAME or $USER or $LNAME or $USERNAME) + "@full.hostname".
        If no username could be found, raise an Abort error.
        """
        user = encoding.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", "username")
            if user is not None:
                user = os.path.expandvars(user)
        if user is None:
            user = encoding.environ.get("EMAIL")
        if user is None and acceptempty:
            return user
        if user is None and self.configbool("ui", "askusername"):
            user = self.prompt(_("enter a commit username:"), default=None)
        if user is None and not self.interactive():
            try:
                user = '%s@%s' % (util.getuser(), socket.getfqdn())
                self.warn(_("no username found, using '%s' instead\n") % user)
            except KeyError:
                pass
        if not user:
            raise error.Abort(_('no username supplied'),
                             hint=_("use 'hg config --edit' "
                                    'to set your username'))
        if "\n" in user:
            raise error.Abort(_("username %s contains a newline\n")
                              % repr(user))
        return user

    def shortuser(self, user):
        """Return a short representation of a user name or email address."""
        if not self.verbose:
            user = util.shortuser(user)
        return user

    def expandpath(self, loc, default=None):
        """Return repository location relative to cwd or from [paths]"""
        try:
            p = self.paths.getpath(loc)
            if p:
                return p.rawloc
        except error.RepoError:
            pass

        if default:
            try:
                p = self.paths.getpath(default)
                if p:
                    return p.rawloc
            except error.RepoError:
                pass

        return loc

    @util.propertycache
    def paths(self):
        return paths(self)

    def pushbuffer(self, error=False, subproc=False, labeled=False):
        """install a buffer to capture standard output of the ui object

        If error is True, the error output will be captured too.

        If subproc is True, output from subprocesses (typically hooks) will be
        captured too.

        If labeled is True, any labels associated with buffered
        output will be handled. By default, this has no effect
        on the output returned, but extensions and GUI tools may
        handle this argument and returned styled output. If output
        is being buffered so it can be captured and parsed or
        processed, labeled should not be set to True.
        """
        self._buffers.append([])
        self._bufferstates.append((error, subproc, labeled))
        self._bufferapplylabels = labeled

    def popbuffer(self):
        '''pop the last buffer and return the buffered output'''
        self._bufferstates.pop()
        if self._bufferstates:
            self._bufferapplylabels = self._bufferstates[-1][2]
        else:
            self._bufferapplylabels = None

        return "".join(self._buffers.pop())

    def write(self, *args, **opts):
        '''write args to output

        By default, this method simply writes to the buffer or stdout.
        Color mode can be set on the UI class to have the output decorated
        with color modifier before being written to stdout.

        The color used is controlled by an optional keyword argument, "label".
        This should be a string containing label names separated by space.
        Label names take the form of "topic.type". For example, ui.debug()
        issues a label of "ui.debug".

        When labeling output for a specific command, a label of
        "cmdname.type" is recommended. For example, status issues
        a label of "status.modified" for modified files.
        '''
        if self._buffers and not opts.get(r'prompt', False):
            if self._bufferapplylabels:
                label = opts.get(r'label', '')
                self._buffers[-1].extend(self.label(a, label) for a in args)
            else:
                self._buffers[-1].extend(args)
        elif self._colormode == 'win32':
            # windows color printing is its own can of crab, defer to
            # the color module and that is it.
            color.win32print(self, self._write, *args, **opts)
        else:
            msgs = args
            if self._colormode is not None:
                label = opts.get(r'label', '')
                msgs = [self.label(a, label) for a in args]
            self._write(*msgs, **opts)

    def _write(self, *msgs, **opts):
        self._progclear()
        # opencode timeblockedsection because this is a critical path
        starttime = util.timer()
        try:
            for a in msgs:
                self.fout.write(a)
        except IOError as err:
            raise error.StdioError(err)
        finally:
            self._blockedtimes['stdio_blocked'] += \
                (util.timer() - starttime) * 1000

    def write_err(self, *args, **opts):
        self._progclear()
        if self._bufferstates and self._bufferstates[-1][0]:
            self.write(*args, **opts)
        elif self._colormode == 'win32':
            # windows color printing is its own can of crab, defer to
            # the color module and that is it.
            color.win32print(self, self._write_err, *args, **opts)
        else:
            msgs = args
            if self._colormode is not None:
                label = opts.get(r'label', '')
                msgs = [self.label(a, label) for a in args]
            self._write_err(*msgs, **opts)

    def _write_err(self, *msgs, **opts):
        try:
            with self.timeblockedsection('stdio'):
                if not getattr(self.fout, 'closed', False):
                    self.fout.flush()
                for a in msgs:
                    self.ferr.write(a)
                # stderr may be buffered under win32 when redirected to files,
                # including stdout.
                if not getattr(self.ferr, 'closed', False):
                    self.ferr.flush()
        except IOError as inst:
            if inst.errno not in (errno.EPIPE, errno.EIO, errno.EBADF):
                raise error.StdioError(inst)

    def flush(self):
        # opencode timeblockedsection because this is a critical path
        starttime = util.timer()
        try:
            try:
                self.fout.flush()
            except IOError as err:
                if err.errno not in (errno.EPIPE, errno.EIO, errno.EBADF):
                    raise error.StdioError(err)
            finally:
                try:
                    self.ferr.flush()
                except IOError as err:
                    if err.errno not in (errno.EPIPE, errno.EIO, errno.EBADF):
                        raise error.StdioError(err)
        finally:
            self._blockedtimes['stdio_blocked'] += \
                (util.timer() - starttime) * 1000

    def _isatty(self, fh):
        if self.configbool('ui', 'nontty'):
            return False
        return util.isatty(fh)

    def disablepager(self):
        self._disablepager = True

    def pager(self, command):
        """Start a pager for subsequent command output.

        Commands which produce a long stream of output should call
        this function to activate the user's preferred pagination
        mechanism (which may be no pager). Calling this function
        precludes any future use of interactive functionality, such as
        prompting the user or activating curses.

        Args:
          command: The full, non-aliased name of the command. That is, "log"
                   not "history, "summary" not "summ", etc.
        """
        if (self._disablepager
            or self.pageractive):
            # how pager should do is already determined
            return

        if not command.startswith('internal-always-') and (
            # explicit --pager=on (= 'internal-always-' prefix) should
            # take precedence over disabling factors below
            command in self.configlist('pager', 'ignore')
            or not self.configbool('ui', 'paginate')
            or not self.configbool('pager', 'attend-' + command, True)
            # TODO: if we want to allow HGPLAINEXCEPT=pager,
            # formatted() will need some adjustment.
            or not self.formatted()
            or self.plain()
            or self._buffers
            # TODO: expose debugger-enabled on the UI object
            or '--debugger' in pycompat.sysargv):
            # We only want to paginate if the ui appears to be
            # interactive, the user didn't say HGPLAIN or
            # HGPLAINEXCEPT=pager, and the user didn't specify --debug.
            return

        pagercmd = self.config('pager', 'pager', rcutil.fallbackpager)
        if not pagercmd:
            return

        pagerenv = {}
        for name, value in rcutil.defaultpagerenv().items():
            if name not in encoding.environ:
                pagerenv[name] = value

        self.debug('starting pager for command %r\n' % command)
        self.flush()

        wasformatted = self.formatted()
        if util.safehasattr(signal, "SIGPIPE"):
            signal.signal(signal.SIGPIPE, _catchterm)
        if self._runpager(pagercmd, pagerenv):
            self.pageractive = True
            # Preserve the formatted-ness of the UI. This is important
            # because we mess with stdout, which might confuse
            # auto-detection of things being formatted.
            self.setconfig('ui', 'formatted', wasformatted, 'pager')
            self.setconfig('ui', 'interactive', False, 'pager')

            # If pagermode differs from color.mode, reconfigure color now that
            # pageractive is set.
            cm = self._colormode
            if cm != self.config('color', 'pagermode', cm):
                color.setup(self)
        else:
            # If the pager can't be spawned in dispatch when --pager=on is
            # given, don't try again when the command runs, to avoid a duplicate
            # warning about a missing pager command.
            self.disablepager()

    def _runpager(self, command, env=None):
        """Actually start the pager and set up file descriptors.

        This is separate in part so that extensions (like chg) can
        override how a pager is invoked.
        """
        if command == 'cat':
            # Save ourselves some work.
            return False
        # If the command doesn't contain any of these characters, we
        # assume it's a binary and exec it directly. This means for
        # simple pager command configurations, we can degrade
        # gracefully and tell the user about their broken pager.
        shell = any(c in command for c in "|&;<>()$`\\\"' \t\n*?[#~=%")

        if pycompat.iswindows and not shell:
            # Window's built-in `more` cannot be invoked with shell=False, but
            # its `more.com` can.  Hide this implementation detail from the
            # user so we can also get sane bad PAGER behavior.  MSYS has
            # `more.exe`, so do a cmd.exe style resolution of the executable to
            # determine which one to use.
            fullcmd = util.findexe(command)
            if not fullcmd:
                self.warn(_("missing pager command '%s', skipping pager\n")
                          % command)
                return False

            command = fullcmd

        try:
            pager = subprocess.Popen(
                command, shell=shell, bufsize=-1,
                close_fds=util.closefds, stdin=subprocess.PIPE,
                stdout=util.stdout, stderr=util.stderr,
                env=util.shellenviron(env))
        except OSError as e:
            if e.errno == errno.ENOENT and not shell:
                self.warn(_("missing pager command '%s', skipping pager\n")
                          % command)
                return False
            raise

        # back up original file descriptors
        stdoutfd = os.dup(util.stdout.fileno())
        stderrfd = os.dup(util.stderr.fileno())

        os.dup2(pager.stdin.fileno(), util.stdout.fileno())
        if self._isatty(util.stderr):
            os.dup2(pager.stdin.fileno(), util.stderr.fileno())

        @self.atexit
        def killpager():
            if util.safehasattr(signal, "SIGINT"):
                signal.signal(signal.SIGINT, signal.SIG_IGN)
            # restore original fds, closing pager.stdin copies in the process
            os.dup2(stdoutfd, util.stdout.fileno())
            os.dup2(stderrfd, util.stderr.fileno())
            pager.stdin.close()
            pager.wait()

        return True

    @property
    def _exithandlers(self):
        return _reqexithandlers

    def atexit(self, func, *args, **kwargs):
        '''register a function to run after dispatching a request

        Handlers do not stay registered across request boundaries.'''
        self._exithandlers.append((func, args, kwargs))
        return func

    def interface(self, feature):
        """what interface to use for interactive console features?

        The interface is controlled by the value of `ui.interface` but also by
        the value of feature-specific configuration. For example:

        ui.interface.histedit = text
        ui.interface.chunkselector = curses

        Here the features are "histedit" and "chunkselector".

        The configuration above means that the default interfaces for commands
        is curses, the interface for histedit is text and the interface for
        selecting chunk is crecord (the best curses interface available).

        Consider the following example:
        ui.interface = curses
        ui.interface.histedit = text

        Then histedit will use the text interface and chunkselector will use
        the default curses interface (crecord at the moment).
        """
        alldefaults = frozenset(["text", "curses"])

        featureinterfaces = {
            "chunkselector": [
                "text",
                "curses",
            ]
        }

        # Feature-specific interface
        if feature not in featureinterfaces.keys():
            # Programming error, not user error
            raise ValueError("Unknown feature requested %s" % feature)

        availableinterfaces = frozenset(featureinterfaces[feature])
        if alldefaults > availableinterfaces:
            # Programming error, not user error. We need a use case to
            # define the right thing to do here.
            raise ValueError(
                "Feature %s does not handle all default interfaces" %
                feature)

        if self.plain():
            return "text"

        # Default interface for all the features
        defaultinterface = "text"
        i = self.config("ui", "interface")
        if i in alldefaults:
            defaultinterface = i

        choseninterface = defaultinterface
        f = self.config("ui", "interface.%s" % feature)
        if f in availableinterfaces:
            choseninterface = f

        if i is not None and defaultinterface != i:
            if f is not None:
                self.warn(_("invalid value for ui.interface: %s\n") %
                          (i,))
            else:
                self.warn(_("invalid value for ui.interface: %s (using %s)\n") %
                         (i, choseninterface))
        if f is not None and choseninterface != f:
            self.warn(_("invalid value for ui.interface.%s: %s (using %s)\n") %
                      (feature, f, choseninterface))

        return choseninterface

    def interactive(self):
        '''is interactive input allowed?

        An interactive session is a session where input can be reasonably read
        from `sys.stdin'. If this function returns false, any attempt to read
        from stdin should fail with an error, unless a sensible default has been
        specified.

        Interactiveness is triggered by the value of the `ui.interactive'
        configuration variable or - if it is unset - when `sys.stdin' points
        to a terminal device.

        This function refers to input only; for output, see `ui.formatted()'.
        '''
        i = self.configbool("ui", "interactive")
        if i is None:
            # some environments replace stdin without implementing isatty
            # usually those are non-interactive
            return self._isatty(self.fin)

        return i

    def termwidth(self):
        '''how wide is the terminal in columns?
        '''
        if 'COLUMNS' in encoding.environ:
            try:
                return int(encoding.environ['COLUMNS'])
            except ValueError:
                pass
        return scmutil.termsize(self)[0]

    def formatted(self):
        '''should formatted output be used?

        It is often desirable to format the output to suite the output medium.
        Examples of this are truncating long lines or colorizing messages.
        However, this is not often not desirable when piping output into other
        utilities, e.g. `grep'.

        Formatted output is triggered by the value of the `ui.formatted'
        configuration variable or - if it is unset - when `sys.stdout' points
        to a terminal device. Please note that `ui.formatted' should be
        considered an implementation detail; it is not intended for use outside
        Mercurial or its extensions.

        This function refers to output only; for input, see `ui.interactive()'.
        This function always returns false when in plain mode, see `ui.plain()'.
        '''
        if self.plain():
            return False

        i = self.configbool("ui", "formatted")
        if i is None:
            # some environments replace stdout without implementing isatty
            # usually those are non-interactive
            return self._isatty(self.fout)

        return i

    def _readline(self, prompt=''):
        if self._isatty(self.fin):
            try:
                # magically add command line editing support, where
                # available
                import readline
                # force demandimport to really load the module
                readline.read_history_file
                # windows sometimes raises something other than ImportError
            except Exception:
                pass

        # call write() so output goes through subclassed implementation
        # e.g. color extension on Windows
        self.write(prompt, prompt=True)
        self.flush()

        # prompt ' ' must exist; otherwise readline may delete entire line
        # - http://bugs.python.org/issue12833
        with self.timeblockedsection('stdio'):
            line = util.bytesinput(self.fin, self.fout, r' ')

        # When stdin is in binary mode on Windows, it can cause
        # raw_input() to emit an extra trailing carriage return
        if pycompat.oslinesep == '\r\n' and line and line[-1] == '\r':
            line = line[:-1]
        return line

    def prompt(self, msg, default="y"):
        """Prompt user with msg, read response.
        If ui is not interactive, the default is returned.
        """
        if not self.interactive():
            self.write(msg, ' ', default or '', "\n")
            return default
        try:
            r = self._readline(self.label(msg, 'ui.prompt'))
            if not r:
                r = default
            if self.configbool('ui', 'promptecho'):
                self.write(r, "\n")
            return r
        except EOFError:
            raise error.ResponseExpected()

    @staticmethod
    def extractchoices(prompt):
        """Extract prompt message and list of choices from specified prompt.

        This returns tuple "(message, choices)", and "choices" is the
        list of tuple "(response character, text without &)".

        >>> ui.extractchoices(b"awake? $$ &Yes $$ &No")
        ('awake? ', [('y', 'Yes'), ('n', 'No')])
        >>> ui.extractchoices(b"line\\nbreak? $$ &Yes $$ &No")
        ('line\\nbreak? ', [('y', 'Yes'), ('n', 'No')])
        >>> ui.extractchoices(b"want lots of $$money$$?$$Ye&s$$N&o")
        ('want lots of $$money$$?', [('s', 'Yes'), ('o', 'No')])
        """

        # Sadly, the prompt string may have been built with a filename
        # containing "$$" so let's try to find the first valid-looking
        # prompt to start parsing. Sadly, we also can't rely on
        # choices containing spaces, ASCII, or basically anything
        # except an ampersand followed by a character.
        m = re.match(br'(?s)(.+?)\$\$([^\$]*&[^ \$].*)', prompt)
        msg = m.group(1)
        choices = [p.strip(' ') for p in m.group(2).split('$$')]
        def choicetuple(s):
            ampidx = s.index('&')
            return s[ampidx + 1:ampidx + 2].lower(), s.replace('&', '', 1)
        return (msg, [choicetuple(s) for s in choices])

    def promptchoice(self, prompt, default=0):
        """Prompt user with a message, read response, and ensure it matches
        one of the provided choices. The prompt is formatted as follows:

           "would you like fries with that (Yn)? $$ &Yes $$ &No"

        The index of the choice is returned. Responses are case
        insensitive. If ui is not interactive, the default is
        returned.
        """

        msg, choices = self.extractchoices(prompt)
        resps = [r for r, t in choices]
        while True:
            r = self.prompt(msg, resps[default])
            if r.lower() in resps:
                return resps.index(r.lower())
            self.write(_("unrecognized response\n"))

    def getpass(self, prompt=None, default=None):
        if not self.interactive():
            return default
        try:
            self.write_err(self.label(prompt or _('password: '), 'ui.prompt'))
            # disable getpass() only if explicitly specified. it's still valid
            # to interact with tty even if fin is not a tty.
            with self.timeblockedsection('stdio'):
                if self.configbool('ui', 'nontty'):
                    l = self.fin.readline()
                    if not l:
                        raise EOFError
                    return l.rstrip('\n')
                else:
                    return getpass.getpass('')
        except EOFError:
            raise error.ResponseExpected()
    def status(self, *msg, **opts):
        '''write status message to output (if ui.quiet is False)

        This adds an output label of "ui.status".
        '''
        if not self.quiet:
            opts[r'label'] = opts.get(r'label', '') + ' ui.status'
            self.write(*msg, **opts)
    def warn(self, *msg, **opts):
        '''write warning message to output (stderr)

        This adds an output label of "ui.warning".
        '''
        opts[r'label'] = opts.get(r'label', '') + ' ui.warning'
        self.write_err(*msg, **opts)
    def note(self, *msg, **opts):
        '''write note to output (if ui.verbose is True)

        This adds an output label of "ui.note".
        '''
        if self.verbose:
            opts[r'label'] = opts.get(r'label', '') + ' ui.note'
            self.write(*msg, **opts)
    def debug(self, *msg, **opts):
        '''write debug message to output (if ui.debugflag is True)

        This adds an output label of "ui.debug".
        '''
        if self.debugflag:
            opts[r'label'] = opts.get(r'label', '') + ' ui.debug'
            self.write(*msg, **opts)

    def edit(self, text, user, extra=None, editform=None, pending=None,
             repopath=None, action=None):
        if action is None:
            self.develwarn('action is None but will soon be a required '
                           'parameter to ui.edit()')
        extra_defaults = {
            'prefix': 'editor',
            'suffix': '.txt',
        }
        if extra is not None:
            if extra.get('suffix') is not None:
                self.develwarn('extra.suffix is not None but will soon be '
                               'ignored by ui.edit()')
            extra_defaults.update(extra)
        extra = extra_defaults

        if action == 'diff':
            suffix = '.diff'
        elif action:
            suffix = '.%s.hg.txt' % action
        else:
            suffix = extra['suffix']

        rdir = None
        if self.configbool('experimental', 'editortmpinhg'):
            rdir = repopath
        (fd, name) = tempfile.mkstemp(prefix='hg-' + extra['prefix'] + '-',
                                      suffix=suffix,
                                      dir=rdir)
        try:
            f = os.fdopen(fd, r'wb')
            f.write(util.tonativeeol(text))
            f.close()

            environ = {'HGUSER': user}
            if 'transplant_source' in extra:
                environ.update({'HGREVISION': hex(extra['transplant_source'])})
            for label in ('intermediate-source', 'source', 'rebase_source'):
                if label in extra:
                    environ.update({'HGREVISION': extra[label]})
                    break
            if editform:
                environ.update({'HGEDITFORM': editform})
            if pending:
                environ.update({'HG_PENDING': pending})

            editor = self.geteditor()

            self.system("%s \"%s\"" % (editor, name),
                        environ=environ,
                        onerr=error.Abort, errprefix=_("edit failed"),
                        blockedtag='editor')

            f = open(name, r'rb')
            t = util.fromnativeeol(f.read())
            f.close()
        finally:
            os.unlink(name)

        return t

    def system(self, cmd, environ=None, cwd=None, onerr=None, errprefix=None,
               blockedtag=None):
        '''execute shell command with appropriate output stream. command
        output will be redirected if fout is not stdout.

        if command fails and onerr is None, return status, else raise onerr
        object as exception.
        '''
        if blockedtag is None:
            # Long cmds tend to be because of an absolute path on cmd. Keep
            # the tail end instead
            cmdsuffix = cmd.translate(None, _keepalnum)[-85:]
            blockedtag = 'unknown_system_' + cmdsuffix
        out = self.fout
        if any(s[1] for s in self._bufferstates):
            out = self
        with self.timeblockedsection(blockedtag):
            rc = self._runsystem(cmd, environ=environ, cwd=cwd, out=out)
        if rc and onerr:
            errmsg = '%s %s' % (os.path.basename(cmd.split(None, 1)[0]),
                                util.explainexit(rc)[0])
            if errprefix:
                errmsg = '%s: %s' % (errprefix, errmsg)
            raise onerr(errmsg)
        return rc

    def _runsystem(self, cmd, environ, cwd, out):
        """actually execute the given shell command (can be overridden by
        extensions like chg)"""
        return util.system(cmd, environ=environ, cwd=cwd, out=out)

    def traceback(self, exc=None, force=False):
        '''print exception traceback if traceback printing enabled or forced.
        only to call in exception handler. returns true if traceback
        printed.'''
        if self.tracebackflag or force:
            if exc is None:
                exc = sys.exc_info()
            cause = getattr(exc[1], 'cause', None)

            if cause is not None:
                causetb = traceback.format_tb(cause[2])
                exctb = traceback.format_tb(exc[2])
                exconly = traceback.format_exception_only(cause[0], cause[1])

                # exclude frame where 'exc' was chained and rethrown from exctb
                self.write_err('Traceback (most recent call last):\n',
                               ''.join(exctb[:-1]),
                               ''.join(causetb),
                               ''.join(exconly))
            else:
                output = traceback.format_exception(exc[0], exc[1], exc[2])
                data = r''.join(output)
                if pycompat.ispy3:
                    enc = pycompat.sysstr(encoding.encoding)
                    data = data.encode(enc, errors=r'replace')
                self.write_err(data)
        return self.tracebackflag or force

    def geteditor(self):
        '''return editor to use'''
        if pycompat.sysplatform == 'plan9':
            # vi is the MIPS instruction simulator on Plan 9. We
            # instead default to E to plumb commit messages to
            # avoid confusion.
            editor = 'E'
        else:
            editor = 'vi'
        return (encoding.environ.get("HGEDITOR") or
                self.config("ui", "editor", editor))

    @util.propertycache
    def _progbar(self):
        """setup the progbar singleton to the ui object"""
        if (self.quiet or self.debugflag
                or self.configbool('progress', 'disable')
                or not progress.shouldprint(self)):
            return None
        return getprogbar(self)

    def _progclear(self):
        """clear progress bar output if any. use it before any output"""
        if not haveprogbar(): # nothing loaded yet
            return
        if self._progbar is not None and self._progbar.printed:
            self._progbar.clear()

    def progress(self, topic, pos, item="", unit="", total=None):
        '''show a progress message

        By default a textual progress bar will be displayed if an operation
        takes too long. 'topic' is the current operation, 'item' is a
        non-numeric marker of the current position (i.e. the currently
        in-process file), 'pos' is the current numeric position (i.e.
        revision, bytes, etc.), unit is a corresponding unit label,
        and total is the highest expected pos.

        Multiple nested topics may be active at a time.

        All topics should be marked closed by setting pos to None at
        termination.
        '''
        if self._progbar is not None:
            self._progbar.progress(topic, pos, item=item, unit=unit,
                                   total=total)
        if pos is None or not self.configbool('progress', 'debug'):
            return

        if unit:
            unit = ' ' + unit
        if item:
            item = ' ' + item

        if total:
            pct = 100.0 * pos / total
            self.debug('%s:%s %d/%d%s (%4.2f%%)\n'
                     % (topic, item, pos, total, unit, pct))
        else:
            self.debug('%s:%s %d%s\n' % (topic, item, pos, unit))

    def log(self, service, *msg, **opts):
        '''hook for logging facility extensions

        service should be a readily-identifiable subsystem, which will
        allow filtering.

        *msg should be a newline-terminated format string to log, and
        then any values to %-format into that format string.

        **opts currently has no defined meanings.
        '''

    def label(self, msg, label):
        '''style msg based on supplied label

        If some color mode is enabled, this will add the necessary control
        characters to apply such color. In addition, 'debug' color mode adds
        markup showing which label affects a piece of text.

        ui.write(s, 'label') is equivalent to
        ui.write(ui.label(s, 'label')).
        '''
        if self._colormode is not None:
            return color.colorlabel(self, msg, label)
        return msg

    def develwarn(self, msg, stacklevel=1, config=None):
        """issue a developer warning message

        Use 'stacklevel' to report the offender some layers further up in the
        stack.
        """
        if not self.configbool('devel', 'all-warnings'):
            if config is None or not self.configbool('devel', config):
                return
        msg = 'devel-warn: ' + msg
        stacklevel += 1 # get in develwarn
        if self.tracebackflag:
            util.debugstacktrace(msg, stacklevel, self.ferr, self.fout)
            self.log('develwarn', '%s at:\n%s' %
                     (msg, ''.join(util.getstackframes(stacklevel))))
        else:
            curframe = inspect.currentframe()
            calframe = inspect.getouterframes(curframe, 2)
            self.write_err('%s at: %s:%s (%s)\n'
                           % ((msg,) + calframe[stacklevel][1:4]))
            self.log('develwarn', '%s at: %s:%s (%s)\n',
                     msg, *calframe[stacklevel][1:4])
            curframe = calframe = None  # avoid cycles

    def deprecwarn(self, msg, version):
        """issue a deprecation warning

        - msg: message explaining what is deprecated and how to upgrade,
        - version: last version where the API will be supported,
        """
        if not (self.configbool('devel', 'all-warnings')
                or self.configbool('devel', 'deprec-warn')):
            return
        msg += ("\n(compatibility will be dropped after Mercurial-%s,"
                " update your code.)") % version
        self.develwarn(msg, stacklevel=2, config='deprec-warn')

    def exportableenviron(self):
        """The environment variables that are safe to export, e.g. through
        hgweb.
        """
        return self._exportableenviron

    @contextlib.contextmanager
    def configoverride(self, overrides, source=""):
        """Context manager for temporary config overrides
        `overrides` must be a dict of the following structure:
        {(section, name) : value}"""
        backups = {}
        try:
            for (section, name), value in overrides.items():
                backups[(section, name)] = self.backupconfig(section, name)
                self.setconfig(section, name, value, source)
            yield
        finally:
            for __, backup in backups.items():
                self.restoreconfig(backup)
            # just restoring ui.quiet config to the previous value is not enough
            # as it does not update ui.quiet class member
            if ('ui', 'quiet') in overrides:
                self.fixconfig(section='ui')

class paths(dict):
    """Represents a collection of paths and their configs.

    Data is initially derived from ui instances and the config files they have
    loaded.
    """
    def __init__(self, ui):
        dict.__init__(self)

        for name, loc in ui.configitems('paths', ignoresub=True):
            # No location is the same as not existing.
            if not loc:
                continue
            loc, sub = ui.configsuboptions('paths', name)
            self[name] = path(ui, name, rawloc=loc, suboptions=sub)

    def getpath(self, name, default=None):
        """Return a ``path`` from a string, falling back to default.

        ``name`` can be a named path or locations. Locations are filesystem
        paths or URIs.

        Returns None if ``name`` is not a registered path, a URI, or a local
        path to a repo.
        """
        # Only fall back to default if no path was requested.
        if name is None:
            if not default:
                default = ()
            elif not isinstance(default, (tuple, list)):
                default = (default,)
            for k in default:
                try:
                    return self[k]
                except KeyError:
                    continue
            return None

        # Most likely empty string.
        # This may need to raise in the future.
        if not name:
            return None

        try:
            return self[name]
        except KeyError:
            # Try to resolve as a local path or URI.
            try:
                # We don't pass sub-options in, so no need to pass ui instance.
                return path(None, None, rawloc=name)
            except ValueError:
                raise error.RepoError(_('repository %s does not exist') %
                                        name)

_pathsuboptions = {}

def pathsuboption(option, attr):
    """Decorator used to declare a path sub-option.

    Arguments are the sub-option name and the attribute it should set on
    ``path`` instances.

    The decorated function will receive as arguments a ``ui`` instance,
    ``path`` instance, and the string value of this option from the config.
    The function should return the value that will be set on the ``path``
    instance.

    This decorator can be used to perform additional verification of
    sub-options and to change the type of sub-options.
    """
    def register(func):
        _pathsuboptions[option] = (attr, func)
        return func
    return register

@pathsuboption('pushurl', 'pushloc')
def pushurlpathoption(ui, path, value):
    u = util.url(value)
    # Actually require a URL.
    if not u.scheme:
        ui.warn(_('(paths.%s:pushurl not a URL; ignoring)\n') % path.name)
        return None

    # Don't support the #foo syntax in the push URL to declare branch to
    # push.
    if u.fragment:
        ui.warn(_('("#fragment" in paths.%s:pushurl not supported; '
                  'ignoring)\n') % path.name)
        u.fragment = None

    return str(u)

@pathsuboption('pushrev', 'pushrev')
def pushrevpathoption(ui, path, value):
    return value

class path(object):
    """Represents an individual path and its configuration."""

    def __init__(self, ui, name, rawloc=None, suboptions=None):
        """Construct a path from its config options.

        ``ui`` is the ``ui`` instance the path is coming from.
        ``name`` is the symbolic name of the path.
        ``rawloc`` is the raw location, as defined in the config.
        ``pushloc`` is the raw locations pushes should be made to.

        If ``name`` is not defined, we require that the location be a) a local
        filesystem path with a .hg directory or b) a URL. If not,
        ``ValueError`` is raised.
        """
        if not rawloc:
            raise ValueError('rawloc must be defined')

        # Locations may define branches via syntax <base>#<branch>.
        u = util.url(rawloc)
        branch = None
        if u.fragment:
            branch = u.fragment
            u.fragment = None

        self.url = u
        self.branch = branch

        self.name = name
        self.rawloc = rawloc
        self.loc = '%s' % u

        # When given a raw location but not a symbolic name, validate the
        # location is valid.
        if not name and not u.scheme and not self._isvalidlocalpath(self.loc):
            raise ValueError('location is not a URL or path to a local '
                             'repo: %s' % rawloc)

        suboptions = suboptions or {}

        # Now process the sub-options. If a sub-option is registered, its
        # attribute will always be present. The value will be None if there
        # was no valid sub-option.
        for suboption, (attr, func) in _pathsuboptions.iteritems():
            if suboption not in suboptions:
                setattr(self, attr, None)
                continue

            value = func(ui, self, suboptions[suboption])
            setattr(self, attr, value)

    def _isvalidlocalpath(self, path):
        """Returns True if the given path is a potentially valid repository.
        This is its own function so that extensions can change the definition of
        'valid' in this case (like when pulling from a git repo into a hg
        one)."""
        return os.path.isdir(os.path.join(path, '.hg'))

    @property
    def suboptions(self):
        """Return sub-options and their values for this path.

        This is intended to be used for presentation purposes.
        """
        d = {}
        for subopt, (attr, _func) in _pathsuboptions.iteritems():
            value = getattr(self, attr)
            if value is not None:
                d[subopt] = value
        return d

# we instantiate one globally shared progress bar to avoid
# competing progress bars when multiple UI objects get created
_progresssingleton = None

def getprogbar(ui):
    global _progresssingleton
    if _progresssingleton is None:
        # passing 'ui' object to the singleton is fishy,
        # this is how the extension used to work but feel free to rework it.
        _progresssingleton = progress.progbar(ui)
    return _progresssingleton

def haveprogbar():
    return _progresssingleton is not None
