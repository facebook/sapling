# ui.py - user interface bits for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import errno
import getpass
import inspect
import os
import re
import socket
import sys
import tempfile
import traceback

from .i18n import _
from .node import hex

from . import (
    config,
    error,
    formatter,
    progress,
    scmutil,
    util,
)

samplehgrcs = {
    'user':
"""# example user config (see "hg help config" for more info)
[ui]
# name and email, e.g.
# username = Jane Doe <jdoe@example.com>
username =

[extensions]
# uncomment these lines to enable some popular extensions
# (see "hg help extensions" for more info)
#
# pager =
# color =""",

    'cloned':
"""# example repository config (see "hg help config" for more info)
[paths]
default = %s

# path aliases to other clones of this repo in URLs or filesystem paths
# (see "hg help config.paths" for more info)
#
# default-push = ssh://jdoe@example.net/hg/jdoes-fork
# my-fork      = ssh://jdoe@example.net/hg/jdoes-fork
# my-clone     = /home/jdoe/jdoes-clone

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",

    'local':
"""# example repository config (see "hg help config" for more info)
[paths]
# path aliases to other clones of this repo in URLs or filesystem paths
# (see "hg help config.paths" for more info)
#
# default      = http://example.com/hg/example-repo
# default-push = ssh://jdoe@example.net/hg/jdoes-fork
# my-fork      = ssh://jdoe@example.net/hg/jdoes-fork
# my-clone     = /home/jdoe/jdoes-clone

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",

    'global':
"""# example system-wide hg config (see "hg help config" for more info)

[extensions]
# uncomment these lines to enable some popular extensions
# (see "hg help extensions" for more info)
#
# blackbox =
# color =
# pager =""",
}

class ui(object):
    def __init__(self, src=None):
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
        self._ocfg = config.config() # overlay
        self._tcfg = config.config() # trusted
        self._ucfg = config.config() # untrusted
        self._trustusers = set()
        self._trustgroups = set()
        self.callhooks = True

        if src:
            self.fout = src.fout
            self.ferr = src.ferr
            self.fin = src.fin

            self._tcfg = src._tcfg.copy()
            self._ucfg = src._ucfg.copy()
            self._ocfg = src._ocfg.copy()
            self._trustusers = src._trustusers.copy()
            self._trustgroups = src._trustgroups.copy()
            self.environ = src.environ
            self.callhooks = src.callhooks
            self.fixconfig()
        else:
            self.fout = sys.stdout
            self.ferr = sys.stderr
            self.fin = sys.stdin

            # shared read-only environment
            self.environ = os.environ
            # we always trust global config files
            for f in scmutil.rcpath():
                self.readconfig(f, trust=True)

    def copy(self):
        return self.__class__(self)

    def formatter(self, topic, opts):
        return formatter.formatter(self, topic, opts)

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
            fp = open(filename)
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
            root = root or os.getcwd()
            for c in self._tcfg, self._ucfg, self._ocfg:
                for n, p in c.items('paths'):
                    if not p:
                        continue
                    if '%%' in p:
                        self.warn(_("(deprecated '%%' in path %s=%s from %s)\n")
                                  % (n, p, self.configsource('paths', n)))
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
                "report_untrusted", True)
            self.tracebackflag = self.configbool('ui', 'traceback', False)

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

    def _data(self, untrusted):
        return untrusted and self._ucfg or self._tcfg

    def configsource(self, section, name, untrusted=False):
        return self._data(untrusted).source(section, name) or 'none'

    def config(self, section, name, default=None, untrusted=False):
        if isinstance(name, list):
            alternates = name
        else:
            alternates = [name]

        for n in alternates:
            value = self._data(untrusted).get(section, n, None)
            if value is not None:
                name = n
                break
        else:
            value = default

        if self.debugflag and not untrusted and self._reportuntrusted:
            for n in alternates:
                uvalue = self._ucfg.get(section, n)
                if uvalue is not None and uvalue != value:
                    self.debug("ignoring untrusted configuration option "
                               "%s.%s = %s\n" % (section, n, uvalue))
        return value

    def configsuboptions(self, section, name, default=None, untrusted=False):
        """Get a config option and all sub-options.

        Some config options have sub-options that are declared with the
        format "key:opt = value". This method is used to return the main
        option and all its declared sub-options.

        Returns a 2-tuple of ``(option, sub-options)``, where `sub-options``
        is a dict of defined sub-options where keys and values are strings.
        """
        data = self._data(untrusted)
        main = data.get(section, name, default)
        if self.debugflag and not untrusted and self._reportuntrusted:
            uvalue = self._ucfg.get(section, name)
            if uvalue is not None and uvalue != main:
                self.debug('ignoring untrusted configuration option '
                           '%s.%s = %s\n' % (section, name, uvalue))

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

    def configpath(self, section, name, default=None, untrusted=False):
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

    def configbool(self, section, name, default=False, untrusted=False):
        """parse a configuration element as a boolean

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'true', 'yes')
        >>> u.configbool(s, 'true')
        True
        >>> u.setconfig(s, 'false', 'no')
        >>> u.configbool(s, 'false')
        False
        >>> u.configbool(s, 'unknown')
        False
        >>> u.configbool(s, 'unknown', True)
        True
        >>> u.setconfig(s, 'invalid', 'somevalue')
        >>> u.configbool(s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a boolean ('somevalue')
        """

        v = self.config(section, name, None, untrusted)
        if v is None:
            return default
        if isinstance(v, bool):
            return v
        b = util.parsebool(v)
        if b is None:
            raise error.ConfigError(_("%s.%s is not a boolean ('%s')")
                                    % (section, name, v))
        return b

    def configint(self, section, name, default=None, untrusted=False):
        """parse a configuration element as an integer

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'int1', '42')
        >>> u.configint(s, 'int1')
        42
        >>> u.setconfig(s, 'int2', '-42')
        >>> u.configint(s, 'int2')
        -42
        >>> u.configint(s, 'unknown', 7)
        7
        >>> u.setconfig(s, 'invalid', 'somevalue')
        >>> u.configint(s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not an integer ('somevalue')
        """

        v = self.config(section, name, None, untrusted)
        if v is None:
            return default
        try:
            return int(v)
        except ValueError:
            raise error.ConfigError(_("%s.%s is not an integer ('%s')")
                                    % (section, name, v))

    def configbytes(self, section, name, default=0, untrusted=False):
        """parse a configuration element as a quantity in bytes

        Units can be specified as b (bytes), k or kb (kilobytes), m or
        mb (megabytes), g or gb (gigabytes).

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'val1', '42')
        >>> u.configbytes(s, 'val1')
        42
        >>> u.setconfig(s, 'val2', '42.5 kb')
        >>> u.configbytes(s, 'val2')
        43520
        >>> u.configbytes(s, 'unknown', '7 MB')
        7340032
        >>> u.setconfig(s, 'invalid', 'somevalue')
        >>> u.configbytes(s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a byte quantity ('somevalue')
        """

        value = self.config(section, name)
        if value is None:
            if not isinstance(default, str):
                return default
            value = default
        try:
            return util.sizetoint(value)
        except error.ParseError:
            raise error.ConfigError(_("%s.%s is not a byte quantity ('%s')")
                                    % (section, name, value))

    def configlist(self, section, name, default=None, untrusted=False):
        """parse a configuration element as a list of comma/space separated
        strings

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'list1', 'this,is "a small" ,test')
        >>> u.configlist(s, 'list1')
        ['this', 'is', 'a small', 'test']
        """

        def _parse_plain(parts, s, offset):
            whitespace = False
            while offset < len(s) and (s[offset].isspace() or s[offset] == ','):
                whitespace = True
                offset += 1
            if offset >= len(s):
                return None, parts, offset
            if whitespace:
                parts.append('')
            if s[offset] == '"' and not parts[-1]:
                return _parse_quote, parts, offset + 1
            elif s[offset] == '"' and parts[-1][-1] == '\\':
                parts[-1] = parts[-1][:-1] + s[offset]
                return _parse_plain, parts, offset + 1
            parts[-1] += s[offset]
            return _parse_plain, parts, offset + 1

        def _parse_quote(parts, s, offset):
            if offset < len(s) and s[offset] == '"': # ""
                parts.append('')
                offset += 1
                while offset < len(s) and (s[offset].isspace() or
                        s[offset] == ','):
                    offset += 1
                return _parse_plain, parts, offset

            while offset < len(s) and s[offset] != '"':
                if (s[offset] == '\\' and offset + 1 < len(s)
                        and s[offset + 1] == '"'):
                    offset += 1
                    parts[-1] += '"'
                else:
                    parts[-1] += s[offset]
                offset += 1

            if offset >= len(s):
                real_parts = _configlist(parts[-1])
                if not real_parts:
                    parts[-1] = '"'
                else:
                    real_parts[0] = '"' + real_parts[0]
                    parts = parts[:-1]
                    parts.extend(real_parts)
                return None, parts, offset

            offset += 1
            while offset < len(s) and s[offset] in [' ', ',']:
                offset += 1

            if offset < len(s):
                if offset + 1 == len(s) and s[offset] == '"':
                    parts[-1] += '"'
                    offset += 1
                else:
                    parts.append('')
            else:
                return None, parts, offset

            return _parse_plain, parts, offset

        def _configlist(s):
            s = s.rstrip(' ,')
            if not s:
                return []
            parser, parts, offset = _parse_plain, [''], 0
            while parser:
                parser, parts, offset = parser(parts, s, offset)
            return parts

        result = self.config(section, name, untrusted=untrusted)
        if result is None:
            result = default or []
        if isinstance(result, basestring):
            result = _configlist(result.lstrip(' ,\n'))
            if result is None:
                result = default or []
        return result

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
        - True otherwise
        '''
        if 'HGPLAIN' not in os.environ and 'HGPLAINEXCEPT' not in os.environ:
            return False
        exceptions = os.environ.get('HGPLAINEXCEPT', '').strip().split(',')
        if feature and exceptions:
            return feature not in exceptions
        return True

    def username(self):
        """Return default username to be used in commits.

        Searched in this order: $HGUSER, [ui] section of hgrcs, $EMAIL
        and stop searching if one of these is set.
        If not found and ui.askusername is True, ask the user, else use
        ($LOGNAME or $USER or $LNAME or $USERNAME) + "@full.hostname".
        """
        user = os.environ.get("HGUSER")
        if user is None:
            user = self.config("ui", ["username", "user"])
            if user is not None:
                user = os.path.expandvars(user)
        if user is None:
            user = os.environ.get("EMAIL")
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

        By default, this method simply writes to the buffer or stdout,
        but extensions or GUI tools may override this method,
        write_err(), popbuffer(), and label() to style output from
        various parts of hg.

        An optional keyword argument, "label", can be passed in.
        This should be a string containing label names separated by
        space. Label names take the form of "topic.type". For example,
        ui.debug() issues a label of "ui.debug".

        When labeling output for a specific command, a label of
        "cmdname.type" is recommended. For example, status issues
        a label of "status.modified" for modified files.
        '''
        if self._buffers and not opts.get('prompt', False):
            self._buffers[-1].extend(a for a in args)
        else:
            self._progclear()
            for a in args:
                self.fout.write(a)

    def write_err(self, *args, **opts):
        self._progclear()
        try:
            if self._bufferstates and self._bufferstates[-1][0]:
                return self.write(*args, **opts)
            if not getattr(self.fout, 'closed', False):
                self.fout.flush()
            for a in args:
                self.ferr.write(a)
            # stderr may be buffered under win32 when redirected to files,
            # including stdout.
            if not getattr(self.ferr, 'closed', False):
                self.ferr.flush()
        except IOError as inst:
            if inst.errno not in (errno.EPIPE, errno.EIO, errno.EBADF):
                raise

    def flush(self):
        try: self.fout.flush()
        except (IOError, ValueError): pass
        try: self.ferr.flush()
        except (IOError, ValueError): pass

    def _isatty(self, fh):
        if self.configbool('ui', 'nontty', False):
            return False
        return util.isatty(fh)

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

        Consider the following exemple:
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
        i = self.config("ui", "interface", None)
        if i in alldefaults:
            defaultinterface = i

        choseninterface = defaultinterface
        f = self.config("ui", "interface.%s" % feature, None)
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
        i = self.configbool("ui", "interactive", None)
        if i is None:
            # some environments replace stdin without implementing isatty
            # usually those are non-interactive
            return self._isatty(self.fin)

        return i

    def termwidth(self):
        '''how wide is the terminal in columns?
        '''
        if 'COLUMNS' in os.environ:
            try:
                return int(os.environ['COLUMNS'])
            except ValueError:
                pass
        return util.termwidth()

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

        i = self.configbool("ui", "formatted", None)
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

        # instead of trying to emulate raw_input, swap (self.fin,
        # self.fout) with (sys.stdin, sys.stdout)
        oldin = sys.stdin
        oldout = sys.stdout
        sys.stdin = self.fin
        sys.stdout = self.fout
        # prompt ' ' must exist; otherwise readline may delete entire line
        # - http://bugs.python.org/issue12833
        line = raw_input(' ')
        sys.stdin = oldin
        sys.stdout = oldout

        # When stdin is in binary mode on Windows, it can cause
        # raw_input() to emit an extra trailing carriage return
        if os.linesep == '\r\n' and line and line[-1] == '\r':
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

        >>> ui.extractchoices("awake? $$ &Yes $$ &No")
        ('awake? ', [('y', 'Yes'), ('n', 'No')])
        >>> ui.extractchoices("line\\nbreak? $$ &Yes $$ &No")
        ('line\\nbreak? ', [('y', 'Yes'), ('n', 'No')])
        >>> ui.extractchoices("want lots of $$money$$?$$Ye&s$$N&o")
        ('want lots of $$money$$?', [('s', 'Yes'), ('o', 'No')])
        """

        # Sadly, the prompt string may have been built with a filename
        # containing "$$" so let's try to find the first valid-looking
        # prompt to start parsing. Sadly, we also can't rely on
        # choices containing spaces, ASCII, or basically anything
        # except an ampersand followed by a character.
        m = re.match(r'(?s)(.+?)\$\$([^\$]*&[^ \$].*)', prompt)
        msg = m.group(1)
        choices = [p.strip(' ') for p in m.group(2).split('$$')]
        return (msg,
                [(s[s.index('&') + 1].lower(), s.replace('&', '', 1))
                 for s in choices])

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
            if self.configbool('ui', 'nontty'):
                return self.fin.readline().rstrip('\n')
            else:
                return getpass.getpass('')
        except EOFError:
            raise error.ResponseExpected()
    def status(self, *msg, **opts):
        '''write status message to output (if ui.quiet is False)

        This adds an output label of "ui.status".
        '''
        if not self.quiet:
            opts['label'] = opts.get('label', '') + ' ui.status'
            self.write(*msg, **opts)
    def warn(self, *msg, **opts):
        '''write warning message to output (stderr)

        This adds an output label of "ui.warning".
        '''
        opts['label'] = opts.get('label', '') + ' ui.warning'
        self.write_err(*msg, **opts)
    def note(self, *msg, **opts):
        '''write note to output (if ui.verbose is True)

        This adds an output label of "ui.note".
        '''
        if self.verbose:
            opts['label'] = opts.get('label', '') + ' ui.note'
            self.write(*msg, **opts)
    def debug(self, *msg, **opts):
        '''write debug message to output (if ui.debugflag is True)

        This adds an output label of "ui.debug".
        '''
        if self.debugflag:
            opts['label'] = opts.get('label', '') + ' ui.debug'
            self.write(*msg, **opts)

    def edit(self, text, user, extra=None, editform=None, pending=None):
        extra_defaults = {
            'prefix': 'editor',
            'suffix': '.txt',
        }
        if extra is not None:
            extra_defaults.update(extra)
        extra = extra_defaults
        (fd, name) = tempfile.mkstemp(prefix='hg-' + extra['prefix'] + '-',
                                      suffix=extra['suffix'], text=True)
        try:
            f = os.fdopen(fd, "w")
            f.write(text)
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
                        onerr=error.Abort, errprefix=_("edit failed"))

            f = open(name)
            t = f.read()
            f.close()
        finally:
            os.unlink(name)

        return t

    def system(self, cmd, environ=None, cwd=None, onerr=None, errprefix=None):
        '''execute shell command with appropriate output stream. command
        output will be redirected if fout is not stdout.
        '''
        out = self.fout
        if any(s[1] for s in self._bufferstates):
            out = self
        return util.system(cmd, environ=environ, cwd=cwd, onerr=onerr,
                           errprefix=errprefix, out=out)

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
                self.write_err(''.join(output))
        return self.tracebackflag or force

    def geteditor(self):
        '''return editor to use'''
        if sys.platform == 'plan9':
            # vi is the MIPS instruction simulator on Plan 9. We
            # instead default to E to plumb commit messages to
            # avoid confusion.
            editor = 'E'
        else:
            editor = 'vi'
        return (os.environ.get("HGEDITOR") or
                self.config("ui", "editor") or
                os.environ.get("VISUAL") or
                os.environ.get("EDITOR", editor))

    @util.propertycache
    def _progbar(self):
        """setup the progbar singleton to the ui object"""
        if (self.quiet or self.debugflag
                or self.configbool('progress', 'disable', False)
                or not progress.shouldprint(self)):
            return None
        return getprogbar(self)

    def _progclear(self):
        """clear progress bar output if any. use it before any output"""
        if '_progbar' not in vars(self): # nothing loaded yet
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
            self.debug('%s:%s %s/%s%s (%4.2f%%)\n'
                     % (topic, item, pos, total, unit, pct))
        else:
            self.debug('%s:%s %s%s\n' % (topic, item, pos, unit))

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

        Like ui.write(), this just returns msg unchanged, but extensions
        and GUI tools can override it to allow styling output without
        writing it.

        ui.write(s, 'label') is equivalent to
        ui.write(ui.label(s, 'label')).
        '''
        return msg

    def develwarn(self, msg, stacklevel=1):
        """issue a developer warning message

        Use 'stacklevel' to report the offender some layers further up in the
        stack.
        """
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
        self.develwarn(msg, stacklevel=2)

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
        self.loc = str(u)

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
