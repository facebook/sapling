# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# ui.py - user interface bits for mercurial
#
# Copyright 2005-2007 Olivia Mackall <olivia@selenic.com>
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
import time
import traceback
from enum import IntEnum
from typing import Any, Dict, List, Tuple, Union

import bindings
from edenscm import tracing

from . import (
    blackbox,
    color,
    encoding,
    error,
    formatter,
    identity,
    json,
    metrics,
    perftrace,
    progress,
    pycompat,
    rcutil,
    scmutil,
    uiconfig,
    util,
)
from .i18n import _
from .node import hex
from .pycompat import decodeutf8, encodeutf8


urlreq = util.urlreq

samplehgrcs = {
    "user": """# example user config (see '@prog@ help config' for more info)
[ui]
# name and email, e.g.
# username = Jane Doe <jdoe@example.com>
username =

# uncomment to disable color in command output
# (see '@prog@ help color' for details)
# color = never

# uncomment to disable command output pagination
# (see '@prog@ help pager' for details)
# paginate = never
""",
    "cloned": """# example repository config (see '@prog@ help config' for more info)
[paths]
default = %s

# URL aliases to other repo sources
# (see '@prog@ help config.paths' for more info)
#
# my-fork = https://example.com/jdoe/example-repo

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",
    "local": """# example repository config (see '@prog@ help config' for more info)
[paths]
# URL aliases to other repo sources
# (see '@prog@ help config.paths' for more info)
#
# default = https://example.com/example-org/example-repo
# my-fork = ssh://jdoe@example.com/jdoe/example-repo

[ui]
# name and email (local to this repository, optional), e.g.
# username = Jane Doe <jdoe@example.com>
""",
    "system": """# example system-wide @prog@ config (see '@prog@ help config' for more info)

[ui]
# uncomment to disable color in command output
# (see '@prog@ help color' for details)
# color = never

# uncomment to disable command output pagination
# (see '@prog@ help pager' for details)
# paginate = never
""",
}


class httppasswordmgrdbproxy(object):
    """Delays loading urllib2 until it's needed."""

    def __init__(self):
        self._mgr = None

    def _get_mgr(self):
        if self._mgr is None:
            self._mgr = urlreq.httppasswordmgrwithdefaultrealm()
        return self._mgr

    def add_password(self, realm, uris, user, passwd):
        return self._get_mgr().add_password(realm, uris, user, passwd)

    def find_user_password(self, realm, uri):
        return tuple(v for v in self._get_mgr().find_user_password(realm, uri))


def _catchterm(*args):
    raise error.SignalInterrupt


# unique object used to detect no default value has been provided when
# retrieving configuration value.
_unset: object = uiconfig._unset

# _reqexithandlers: callbacks run at the end of a request
_reqexithandlers = []


class deprecationlevel(IntEnum):
    # Logs usage of the deprecated code path
    Log = 0
    # Prints a warning on usage of the deprecated code path
    Warn = 1
    # Inserts a 2 second sleep to the deprecated code path
    Slow = 2
    # Throws an exception, but a config can be used to opt in to the deprecated feature
    Optin = 3
    # Throws a non-bypassable exception
    Block = 4


class ui(object):
    def __init__(self, src=None, rcfg=None):
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
        # Redirect output to an alternative ui object.
        self._outputui = None
        self.callhooks = True
        # Insecure server connections requested.
        self.insecureconnections = False
        # color mode: see color.py for possible value
        self._colormode = None
        self._styler = None
        self._styles = {}
        # Whether the output stream is known to be a terminal.
        self._terminaloutput = None
        # The current command name being executed.
        self.cmdname = None

        # CLI config overrides to allow easier reloading of config.
        self.cliconfigs = []
        self.cliconfigfiles = []
        self.clioptions = {}

        if src:
            self._uiconfig = src._uiconfig.copy()

            self.fout = src.fout
            self.ferr = src.ferr
            self.fin = src.fin
            self.pageractive = src.pageractive
            self._disablepager = src._disablepager
            self._tweaked = src._tweaked
            self._outputui = src._outputui
            self._terminaloutput = src._terminaloutput
            self._correlator = src._correlator

            self.environ = src.environ
            self.callhooks = src.callhooks
            self.insecureconnections = src.insecureconnections
            self._colormode = src._colormode
            self._styler = src._styler
            self._styles = src._styles.copy()

            self.httppasswordmgrdb = src.httppasswordmgrdb
            self._measuredtimes = src._measuredtimes

            self.metrics = src.metrics
            self.cmdname = src.cmdname

            self.cliconfigs = src.cliconfigs.copy()
            self.cliconfigfiles = src.cliconfigfiles.copy()
            self.clioptions = src.clioptions.copy()

            self.identity = src.identity
        else:
            self._uiconfig = uiconfig.uiconfig(rcfg=rcfg)

            self.fout = util.refcell(util.mainio.output())
            self.ferr = util.refcell(util.mainio.error())
            self.fin = util.refcell(util.stdin)
            self.pageractive = False
            self._disablepager = False
            self._tweaked = False
            self._correlator = util.refcell(None)

            # shared read-only environment
            self.environ = encoding.environ

            self.httppasswordmgrdb = httppasswordmgrdbproxy()
            self._measuredtimes = collections.defaultdict(int)

            self.metrics = metrics.metrics(self)

            self.identity = identity.default()

        allowed = self.configlist("experimental", "exportableenviron")
        if "*" in allowed:
            self._exportableenviron = self.environ
        else:
            self._exportableenviron = {}
            for k in allowed:
                if k in self.environ:
                    self._exportableenviron[k] = self.environ[k]

    @classmethod
    def load(cls, repopath=None):
        """Create a ui and load global and user configs"""
        u = cls()
        uiconfig.uiconfig.load(u, repopath)
        return u

    def reloadconfigs(self, repopath=None):
        # repopath should be the non-shared repo path without .hg/
        self._uiconfig.reload(self, repopath)

    def loadrepoconfig(self, repopath):
        """Load repofull config from repopath if not already loaded."""

        # Update identity as we transition from repoless to repofull ui object.
        ident = identity.sniffdir(repopath)
        if ident:
            self.identity = ident

        loadedfiles = self._rcfg.files()
        repohgrc = os.path.join(
            repopath, self.identity.dotdir(), self.identity.configrepofile()
        )

        # Check if our repo hgrc path (or Windows UNC flavor) have already been loaded.
        if not any(lf in {repohgrc, f"\\\\?\\{repohgrc}"} for lf in loadedfiles):
            tracing.debug(
                "reloading config: hgrc %s not in %s" % (repohgrc, loadedfiles),
                target="config",
            )
            self.reloadconfigs(repopath)
        else:
            # Expand "paths" using proper repo root.
            self._uiconfig.fixconfig(root=repopath)

    def copy(self):
        return self.__class__(self)

    def copywithoutrepo(self):
        """Create a copy sans repo-specific config."""

        # This copies the config as well, but uiconfig.load below
        # completely replaces the _uiconfig object.
        repoless = self.copy()
        uiconfig.uiconfig.load(repoless, None)

        repoless.setclioverrides(self.cliconfigs, self.cliconfigfiles)
        repoless.deriveconfigfromclioptions(self.clioptions)
        return repoless

    def resetstate(self):
        """Clear internal state that shouldn't persist across commands"""
        progress.resetstate()
        self.httppasswordmgrdb = httppasswordmgrdbproxy()

    def correlator(self):
        """a random string that is logged on both the client and server.  This
        can be used to correlate the client logging to the server logging.
        """
        assert isinstance(self._correlator, util.refcell)
        if self._correlator.get() is None:
            correlator = bindings.edenapi.correlator()
            self._correlator.swap(correlator)
            self.log("clienttelemetry", client_correlator=correlator)
        return self._correlator.get()

    def setclioverrides(self, cliconfigs, cliconfigfiles):
        self.cliconfigs = (cliconfigs or []).copy()
        self.cliconfigfiles = (cliconfigfiles or []).copy()
        self._uiconfig.setclioverrides(self.cliconfigs, self.cliconfigfiles)

    def deriveconfigfromclioptions(self, options):
        options = self.clioptions = (options or {}).copy()

        get = lambda name: options.get(name, None)

        if get("verbose") or get("debug") or get("quiet"):
            for opt in ("verbose", "debug", "quiet"):
                val = str(bool(get(opt)))
                self.setconfig("ui", opt, val, "--" + opt)

        if get("traceback"):
            self.setconfig("ui", "traceback", "on", "--traceback")

        if get("noninteractive"):
            self.setconfig("ui", "interactive", "off", "-y")

        if get("insecure"):
            self.insecureconnections = True

    @contextlib.contextmanager
    def timeblockedsection(self, key):
        # this is open-coded below - search for timeblockedsection to find them
        starttime = util.timer()
        try:
            yield
        finally:
            self._measuredtimes[key + "_blocked"] += (util.timer() - starttime) * 1000

    @contextlib.contextmanager
    def timesection(self, key):
        starttime = util.timer()
        try:
            yield
        finally:
            self._measuredtimes[key + "_time"] += (util.timer() - starttime) * 1000

    def formatter(self, topic, opts):
        return formatter.formatter(self, self, topic, opts)

    def readconfig(
        self,
        filename,
        root=None,
        trust=False,
        sections=None,
        remap=None,
        source="ui.readconfig",
    ):
        return self._uiconfig.readconfig(filename, root, trust, sections, remap, source)

    def setconfig(self, section, name, value, source=""):
        return self._uiconfig.setconfig(section, name, value, source)

    def configtostring(self):
        return self._uiconfig.configtostring()

    def configsource(self, section, name):
        return self._uiconfig.configsource(section, name)

    def config(self, section, name, default=_unset):
        """return the plain string version of a config"""
        return self._uiconfig.config(section, name, default)

    def configsuboptions(self, section, name, default=_unset):
        """Get a config option and all sub-options.

        Some config options have sub-options that are declared with the
        format "key:opt = value". This method is used to return the main
        option and all its declared sub-options.

        Returns a 2-tuple of ``(option, sub-options)``, where `sub-options``
        is a dict of defined sub-options where keys and values are strings.
        """
        return self._uiconfig.configsuboptions(section, name, default)

    def configpath(self, section, name, default=_unset):
        "get a path config item, expanded relative to repo root or config file"
        return self._uiconfig.configpath(section, name, default)

    def configbool(self, section, name, default=_unset):
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
        >>> try: u.configbool(s, 'invalid')
        ... except Exception as e: print(e)
        foo.invalid is not a boolean ('somevalue')
        """
        return self._uiconfig.configbool(section, name, default)

    def configwith(self, convert, section, name, default=_unset, desc=None):
        """parse a configuration element with a conversion function

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'float1', '42')
        >>> u.configwith(float, s, 'float1')
        42.0
        >>> u.setconfig(s, 'float2', '-4.25')
        >>> u.configwith(float, s, 'float2')
        -4.25
        >>> u.configwith(float, s, 'unknown', 7)
        7.0
        >>> u.setconfig(s, 'invalid', 'somevalue')
        >>> try: u.configwith(float, s, 'invalid')
        ... except Exception as e: print(e)
        foo.invalid is not a valid float ('somevalue')
        >>> try: u.configwith(float, s, 'invalid', desc='womble')
        ... except Exception as e: print(e)
        foo.invalid is not a valid womble ('somevalue')
        """
        return self._uiconfig.configwith(convert, section, name, default, desc)

    def configint(self, section, name, default=_unset):
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
        >>> try: u.configint(s, 'invalid')
        ... except Exception as e: print(e)
        foo.invalid is not a valid integer ('somevalue')
        """
        return self._uiconfig.configint(section, name, default)

    def configbytes(self, section, name, default=_unset):
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
        >>> try: u.configbytes(s, 'invalid')
        ... except Exception as e: print(e)
        foo.invalid is not a byte quantity ('somevalue')
        """
        return self._uiconfig.configbytes(section, name, default)

    def configlist(self, section, name, default=_unset):
        """parse a configuration element as a list of comma/space separated
        strings

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'list1', 'this,is "a small" ,test')
        >>> u.configlist(s, 'list1')
        ['this', 'is', 'a small', 'test']
        >>> u.setconfig(s, 'list2', 'this, is "a small" , test ')
        >>> u.configlist(s, 'list2')
        ['this', 'is', 'a small', 'test']
        """
        return self._uiconfig.configlist(section, name, default)

    def configdate(self, section, name, default=_unset):
        """parse a configuration element as a tuple of ints

        >>> u = ui(); s = 'foo'
        >>> u.setconfig(s, 'date', '0 0')
        >>> u.configdate(s, 'date')
        (0, 0)
        """
        return self._uiconfig.configdate(section, name, default)

    def hasconfig(self, section, name):
        return self._uiconfig.hasconfig(section, name)

    def has_section(self, section):
        """tell whether section exists in config."""
        return self._uiconfig.has_section(section)

    def configsections(self):
        return self._uiconfig.configsections()

    def configitems(self, section, ignoresub=False):
        return self._uiconfig.configitems(section, ignoresub)

    def walkconfig(self):
        return self._uiconfig.walkconfig()

    def plain(self, feature=None):
        """is plain mode active?

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
        """
        plain = bindings.identity.envvar("PLAIN")
        plainexcept = bindings.identity.envvar("PLAINEXCEPT")
        if plain is None and plainexcept is None:
            return False
        exceptions = (plainexcept or "").strip().split(",")
        # TODO: add support for HGPLAIN=+feature,-feature syntax
        if "+strictflags" not in (plain or "").split(","):
            exceptions.append("strictflags")
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
                user = "%s@%s" % (util.getuser(), socket.getfqdn())
                self.warn(_("no username found, using '%s' instead\n") % user)
            except KeyError:
                pass
        if not user:
            raise error.Abort(
                _("no username supplied"),
                hint=_(
                    'use `@prog@ config --user ui.username "First Last <me@example.com>"` to set your username'
                ),
            )
        if "\n" in user:
            raise error.Abort(_("username %s contains a newline\n") % repr(user))
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

    def popbuffer(self) -> str:
        """pop the last buffer and return the buffered output

        Throws if any element of the buffer is not str.
        """
        self._bufferstates.pop()
        if self._bufferstates:
            self._bufferapplylabels = self._bufferstates[-1][2]
        else:
            self._bufferapplylabels = None

        buf = self._buffers.pop()
        if any(not isinstance(s, str) for s in buf):
            raise error.ProgrammingError("popbuffer cannot be used on bytes buffer")
        return "".join(buf)

    def popbufferbytes(self) -> bytes:
        """pop the last buffer and return the buffered output

        Throws if any element of the buffer is not bytes.
        """
        self._bufferstates.pop()
        if self._bufferstates:
            self._bufferapplylabels = self._bufferstates[-1][2]
        else:
            self._bufferapplylabels = None

        buf = self._buffers.pop()
        if any(not isinstance(s, bytes) for s in buf):
            raise error.ProgrammingError("popbufferbytes cannot be used on str buffer")
        return b"".join(buf)

    def popbufferlist(self) -> "List[Union[str, bytes]]":
        """pop the last buffer and return the buffered output as a list

        May contain both str and bytes.
        """
        self._bufferstates.pop()
        if self._bufferstates:
            self._bufferapplylabels = self._bufferstates[-1][2]
        else:
            self._bufferapplylabels = None

        return self._buffers.pop()

    def _addprefixesandlabels(
        self,
        args: "Tuple[str, ...]",
        opts: "Dict[str, Any]",
        addlabels: bool,
        usebytes: bool = False,
    ) -> "List[str]":
        msgs = []
        for item in r"error", r"notice", r"component":
            itemvalue = opts.get(item)
            if itemvalue:
                itemvalue = "%s:" % itemvalue
                if addlabels:
                    itemvalue = self.label(
                        itemvalue, "ui.prefix.%s" % item, usebytes=usebytes
                    )
                msgs.extend((itemvalue, " "))
        msgs.extend(args)
        if addlabels:
            label = opts.get(r"label", "")
            msgs = [self.label(m, label, usebytes=usebytes) for m in msgs]
        return msgs

    def write(self, *args: str, **opts: "Any") -> None:
        """write args to output

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

        The output can optionally be prefixed by an error prefix, warning prefix
        note prefix, or a component name if the corresponding keyword argument
        is set.  The prefix will be labelled with the "ui.prefix.PREFIXNAME"
        label.
        """
        if self._outputui is not None and not opts.get(r"prompt", False):
            self._outputui.write(*args, **opts)
        elif self._buffers and not opts.get(r"prompt", False):
            msgs = self._addprefixesandlabels(args, opts, bool(self._bufferapplylabels))
            self._buffers[-1].extend(msgs)
        else:
            if self.formatted:
                # Convert arguments from local encoding to output encoding
                # if these encodings differ (e.g. Python 2.7 on Windows).
                # pyre-fixme[9]: Unable to unpack `List[typing.Any]`, expected a tuple.
                args = [encoding.localtooutput(arg) for arg in args]
            msgs = self._addprefixesandlabels(args, opts, bool(self._colormode))
            self._write(*msgs)

    def _write(self, *msgs: str) -> None:
        starttime = util.timer()
        try:
            self.fout.write(encodeutf8("".join(msgs)))
        except IOError as err:
            raise error.StdioError(err)
        finally:
            # Assuming the only way to be blocked on stdout is the pager.
            seconds = util.timer() - starttime
            # Using util.traced is in theory correct, but will generate too
            # many (noisy) tracing events. Only log blocking events that
            # takes some time (ex. 0.1s).
            if seconds >= 0.1:
                util.info("stdio", cat="blocked-after", millis=int(seconds * 1000))
            self._measuredtimes["stdio_blocked"] += (seconds) * 1000

    def writebytes(self, *args, **opts):
        """Like `write` but taking bytes instead of str as arguments.

        Can be used only when we're outputing the file contents to stdout,
        for example in diff, cat, or blame commands.
        """
        if self._outputui is not None and not opts.get(r"prompt", False):
            self._outputui.writebytes(*args, **opts)
        elif self._buffers and not opts.get(r"prompt", False):
            msgs = self._addprefixesandlabels(
                args, opts, self._bufferapplylabels, usebytes=True
            )
            self._buffers[-1].extend(msgs)
        else:
            msgs = self._addprefixesandlabels(
                args, opts, self._colormode, usebytes=True
            )
            self._writebytes(*msgs, **opts)

    def _writebytes(self, *msgs, **opts):
        starttime = util.timer()
        try:
            self.fout.write(b"".join(msgs))
        except IOError as err:
            raise error.StdioError(err)
        finally:
            # Assuming the only way to be blocked on stdout is the pager.
            millis = int((util.timer() - starttime) * 1000)
            if millis >= 20:
                util.info("stdio", cat="blocked-after", millis=millis)
            self._measuredtimes["stdio_blocked"] += millis

    def write_err(self, *args, **opts):
        if self._outputui is not None or (
            self._bufferstates and self._bufferstates[-1][0]
        ):
            self.write(*args, **opts)
        else:
            msgs = self._addprefixesandlabels(args, opts, self._colormode)
            self._write_err(*msgs, **opts)

    def _write_err(self, *msgs, **opts):
        starttime = util.timer()
        try:
            if not getattr(self.fout, "closed", False):
                self.fout.flush()
            # Write all messages in a single operation as stderr may be
            # unbuffered.
            self.ferr.write(encodeutf8("".join(msgs)))
            # stderr may be buffered under win32 when redirected to files,
            # including stdout.
            if not getattr(self.ferr, "closed", False):
                self.ferr.flush()
        except IOError as inst:
            if inst.errno not in (errno.EPIPE, errno.EIO, errno.EBADF):
                raise error.StdioError(inst)
        finally:
            # Assuming the only way to be blocked on stdout is the pager.
            millis = int((util.timer() - starttime) * 1000)
            if millis >= 20:
                util.info("stdio", cat="blocked-after", millis=millis)
            self._measuredtimes["stdio_blocked"] += millis

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
            millis = int((util.timer() - starttime) * 1000)
            self._measuredtimes["stdio_blocked"] += millis
            if millis >= 20:
                util.info("stdio", cat="blocked-after", millis=millis)

    def _isatty(self, fh):
        if self.configbool("ui", "nontty"):
            return False
        if self.configbool("ui", "assume-tty"):
            return True
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
        if self._disablepager or self.pageractive:
            # how pager should do is already determined
            return

        if not command.startswith("internal-always-") and (
            # explicit --pager=on (= 'internal-always-' prefix) should
            # take precedence over disabling factors below
            command in self.configlist("pager", "ignore")
            or not self.configbool("ui", "paginate")
            or not self.configbool("pager", "attend-" + command, True)
            or not self.terminaloutput()
            or self.plain("pager")
            or self._buffers
            # TODO: expose debugger-enabled on the UI object
            or "--debugger" in pycompat.sysargv
        ):
            # We only want to paginate if the ui appears to be
            # interactive, the user didn't say HGPLAIN or
            # HGPLAINEXCEPT=pager, and the user didn't specify --debug.
            return

        pagercmd = self.config("pager", "pager")
        if not pagercmd:
            return

        pagerenv = {}
        for name, value in rcutil.defaultpagerenv().items():
            if name not in encoding.environ:
                pagerenv[name] = value

        # Tell the pager what encoding we're sending it.
        pagerencoding = self.config("pager", "encoding")
        if pagerencoding:
            pagerenv["LESSCHARSET"] = pagerencoding

        self.debug("starting pager for command %r\n" % command)
        self.flush()

        wasformatted = self.formatted
        wasterminaloutput = self.terminaloutput()
        if util.safehasattr(signal, "SIGPIPE"):
            util.signal(signal.SIGPIPE, _catchterm)
        if pagercmd == "internal:streampager":
            self._runinternalstreampager()
        elif self._runpager(pagercmd, pagerenv):
            self.pageractive = True
            # Preserve the formatted-ness of the UI. This is important
            # because we mess with stdout, which might confuse
            # auto-detection of things being formatted.
            self.setconfig("ui", "formatted", wasformatted, "pager")
            util.clearcachedproperty(self, "formatted")
            self.setconfig("ui", "interactive", False, "pager")
            self._terminaloutput = wasterminaloutput

            # If pager encoding is set, update the output encoding
            if pagerencoding:
                encoding.outputencoding = pagerencoding
        else:
            # If the pager can't be spawned in dispatch when --pager=on is
            # given, don't try again when the command runs, to avoid a duplicate
            # warning about a missing pager command.
            self.disablepager()

    def _runinternalstreampager(self):
        """Start the builtin streampager"""
        origencoding = encoding.outputencoding
        self.flush()

        # This will start the pager using the system terminal immediately.
        util.mainio.start_pager(self._rcfg)

        # The Rust pager wants utf-8 unconditionally.
        encoding.outputencoding = "utf-8"

        @self.atexit
        def waitpager():
            with self.timeblockedsection("pager"):
                util.mainio.wait_pager()
            encoding.outputencoding = origencoding

        self.pageractive = True

    def _runpager(self, command, env=None):
        """Actually start the pager and set up file descriptors.

        This is separate in part so that extensions (like chg) can
        override how a pager is invoked.
        """
        if command == "cat":
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
                self.warn(_("missing pager command '%s', skipping pager\n") % command)
                return False

            command = fullcmd

        try:
            with self.timeblockedsection("pager"):
                util.mainio.disable_progress()
                pager = subprocess.Popen(
                    command,
                    shell=shell,
                    bufsize=-1,
                    close_fds=util.closefds,
                    stdin=subprocess.PIPE,
                    stdout=util.stdout,
                    stderr=util.stderr,
                    env=util.shellenviron(env),
                )
        except OSError as e:
            if e.errno == errno.ENOENT and not shell:
                self.warn(_("missing pager command '%s', skipping pager\n") % command)
                return False
            raise

        # back up original file descriptors
        stdoutfd = os.dup(util.stdout.fileno())
        stderrfd = os.dup(util.stderr.fileno())

        os.dup2(pager.stdin.fileno(), util.stdout.fileno())
        if self._isatty(util.stderr) and self.configbool("pager", "stderr"):
            os.dup2(pager.stdin.fileno(), util.stderr.fileno())

        @self.atexit
        def killpager():
            if util.safehasattr(signal, "SIGINT"):
                util.signal(signal.SIGINT, signal.SIG_IGN)
            # restore original fds, closing pager.stdin copies in the process
            os.dup2(stdoutfd, util.stdout.fileno())
            os.dup2(stderrfd, util.stderr.fileno())
            pager.stdin.close()
            with self.timeblockedsection("pager"):
                pager.wait()

        return True

    @property
    def _exithandlers(self):
        return _reqexithandlers

    def atexit(self, func, *args, **kwargs):
        """register a function to run after dispatching a request

        Handlers do not stay registered across request boundaries."""
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

        featureinterfaces = {"chunkselector": ["text", "curses"]}

        # Feature-specific interface
        if feature not in featureinterfaces.keys():
            # Programming error, not user error
            raise ValueError("Unknown feature requested %s" % feature)

        availableinterfaces = frozenset(featureinterfaces[feature])
        if alldefaults > availableinterfaces:
            # Programming error, not user error. We need a use case to
            # define the right thing to do here.
            raise ValueError(
                "Feature %s does not handle all default interfaces" % feature
            )

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
                self.warn(_("invalid value for ui.interface: %s\n") % (i,))
            else:
                self.warn(
                    _("invalid value for ui.interface: %s (using %s)\n")
                    % (i, choseninterface)
                )
        if f is not None and choseninterface != f:
            self.warn(
                _("invalid value for ui.interface.%s: %s (using %s)\n")
                % (feature, f, choseninterface)
            )

        return choseninterface

    def interactive(self):
        """is interactive input allowed?

        An interactive session is a session where input can be reasonably read
        from `sys.stdin'. If this function returns false, any attempt to read
        from stdin should fail with an error, unless a sensible default has been
        specified.

        Interactiveness is triggered by the value of the `ui.interactive'
        configuration variable or - if it is unset - when `sys.stdin' points
        to a terminal device.

        This function refers to input only; for output, see `ui.formatted()'.
        """
        i = self.configbool("ui", "interactive")
        if i is None:
            # some environments replace stdin without implementing isatty
            # usually those are non-interactive
            return self._isatty(self.fin)

        return i

    def termwidth(self):
        """how wide is the terminal in columns?"""
        if "COLUMNS" in encoding.environ:
            try:
                return int(encoding.environ["COLUMNS"])
            except ValueError:
                pass
        return scmutil.termsize(self)[0]

    def terminaloutput(self):
        """is output to a terminal?"""
        istty = self._terminaloutput
        if istty is None:
            return self._isatty(self.fout)
        return istty

    @util.propertycache
    def formatted(self):
        """should formatted output be used?

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
        """
        if self.plain():
            return False

        i = self.configbool("ui", "formatted")
        if i is None:
            # some environments replace stdout without implementing isatty
            # usually those are non-interactive
            return self._isatty(self.fout)

        return i

    def _readline(self, prompt=""):
        usereadline = self._isatty(self.fin) and self._isatty(self.fout)
        if usereadline:
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
        self.write(prompt + " ", prompt=True)
        self.flush()

        # prompt ' ' must exist; otherwise readline may delete entire line
        # - http://bugs.python.org/issue12833
        with self.timeblockedsection("stdio"):
            if usereadline:
                line = pycompat.rawinput("")
            else:
                line = pycompat.decodeutf8(self.fin.readline())
                if not line:
                    raise EOFError
                line = line.rstrip(pycompat.oslinesep)

        # When stdin is in binary mode on Windows, it can cause
        # raw_input() to emit an extra trailing carriage return
        if pycompat.oslinesep == "\r\n" and line and line[-1] == "\r":
            line = line[:-1]
        return line

    def prompt(self, msg, default="y"):
        """Prompt user with msg, read response.
        If ui is not interactive, the default is returned.
        """
        if not self.interactive():
            self.write(msg, " ", default or "", "\n")
            return default
        try:
            with progress.suspend(), util.traced("prompt", cat="blocked"):
                r = self._readline(self.label(msg, "ui.prompt"))
                if not r:
                    r = default
                if self.configbool("ui", "promptecho"):
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
        >>> ui.extractchoices("which commit to move to [1-10/(c)ancel]? $$ &cancel $$ &1 $$ &2 $$ &3 $$ &4 $$ &5 $$ &6 $$ &7 $$ &8 $$ &9 $$ &10")
        ('which commit to move to [1-10/(c)ancel]? ', [('c', 'cancel'), ('1', '1'), ('2', '2'), ('3', '3'), ('4', '4'), ('5', '5'), ('6', '6'), ('7', '7'), ('8', '8'), ('9', '9'), ('10', '10')])
        """

        # Sadly, the prompt string may have been built with a filename
        # containing "$$" so let's try to find the first valid-looking
        # prompt to start parsing. Sadly, we also can't rely on
        # choices containing spaces, ASCII, or basically anything
        # except an ampersand followed by a character.
        m = re.match(r"(?s)(.+?)\$\$([^\$]*&[^ \$].*)", prompt)
        msg = m.group(1)
        choices = [p.strip(" ") for p in m.group(2).split("$$")]

        def choicetuple(s):
            if (choice := s.replace("&", "", 1)).isdecimal():
                return choice, choice
            ampidx = s.index("&")
            return s[ampidx + 1 : ampidx + 2].lower(), s.replace("&", "", 1)

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
        with progress.suspend():
            while True:
                r = self.prompt(msg, resps[default])
                if r.lower() in resps:
                    return resps.index(r.lower())
                self.write(_("unrecognized response\n"))

    def getpass(self, prompt=None, default=None):
        if not self.interactive():
            return default
        try:
            self.write_err(self.label(prompt or _("password: "), "ui.prompt"))
            # disable getpass() only if explicitly specified. it's still valid
            # to interact with tty even if fin is not a tty.
            with self.timeblockedsection("stdio"):
                if self.configbool("ui", "nontty"):
                    l = decodeutf8(self.fin.readline())
                    if not l:
                        raise EOFError
                    return l.rstrip("\n")
                else:
                    return getpass.getpass("")
        except EOFError:
            raise error.ResponseExpected()

    def status(self, *msg, **opts):
        """write status message to output (if ui.quiet is False)

        This adds an output label of "ui.status".
        """
        if not self.quiet:
            opts[r"label"] = opts.get(r"label", "") + " ui.status"
            self.write(*msg, **opts)

    def status_err(self, *msg, **opts):
        """write status message to ferr (if ui.quiet is False)

        This adds an output label of "ui.status".
        """
        if not self.quiet:
            opts[r"label"] = opts.get(r"label", "") + " ui.status"
            self.write_err(*msg, **opts)

    def warn(self, *msg, **opts):
        """write warning message to output (stderr)

        This adds an output label of "ui.warning".
        """
        opts[r"label"] = opts.get(r"label", "") + " ui.warning"
        self.write_err(*msg, **opts)

    def note(self, *msg, **opts):
        """write note to output (if ui.verbose is True)

        This adds an output label of "ui.note".
        """
        if self.verbose:
            opts[r"label"] = opts.get(r"label", "") + " ui.note"
            self.write(*msg, **opts)

    def note_err(self, *msg, **opts):
        """write note to ferr (if ui.verbose is True)

        This adds an output label of "ui.note".
        """
        if self.verbose:
            opts[r"label"] = opts.get(r"label", "") + " ui.note"
            self.write_err(*msg, **opts)

    def debug(self, *msg, **opts):
        """write debug message to output (if ui.debugflag is True)

        This adds an output label of "ui.debug".
        """
        msg = "".join(msg)
        if self.debugflag:
            opts[r"label"] = opts.get(r"label", "") + " ui.debug"
            self.write_err(msg, **opts)
        tracing.debug(msg.rstrip("\n"), depth=1)

    def edit(
        self,
        text,
        user,
        extra=None,
        editform=None,
        pending=None,
        sharedpending=None,
        repopath=None,
        action=None,
    ):
        if action is None:
            self.develwarn(
                "action is None but will soon be a required " "parameter to ui.edit()"
            )
        extra_defaults = {"prefix": "editor", "suffix": ".txt"}
        if extra is not None:
            if extra.get("suffix") is not None:
                self.develwarn(
                    "extra.suffix is not None but will soon be " "ignored by ui.edit()"
                )
            extra_defaults.update(extra)
        extra = extra_defaults

        if action == "diff":
            suffix = ".diff"
        elif action:
            suffix = ".%s.hg.txt" % action
        else:
            suffix = extra["suffix"]

        rdir = repopath
        if rdir:
            # Create a "edit-tmp" directory on demand. So that directory only
            # contains temporary editor files and we can GC them.
            rdir = os.path.join(rdir, "edit-tmp")
            util.makedirs(rdir)
        (fd, name) = tempfile.mkstemp(
            prefix="hg-" + extra["prefix"] + "-", suffix=suffix, dir=rdir
        )
        try:
            f = util.fdopen(fd, r"wb")
            f.write(encodeutf8(util.tonativeeol(text)))
            f.close()

            environ = {"HGUSER": user}
            if "transplant_source" in extra:
                environ.update({"HGREVISION": hex(extra["transplant_source"])})
            for label in ("intermediate-source", "source", "rebase_source"):
                if label in extra:
                    environ.update({"HGREVISION": extra[label]})
                    break
            if editform:
                environ.update({"HGEDITFORM": editform})
            if pending:
                environ.update({"HG_PENDING": pending})
            if sharedpending:
                environ.update({"HG_SHAREDPENDING": sharedpending})

            editor = self.geteditor()
            if not editor:
                raise error.ProgrammingError("editor is not defined")

            # Special cases to avoid shelling out
            if editor == "internal:none":
                pass
            elif editor == "cat":
                # Print the text
                self.write(text)
            elif editor == "cat>":
                # Read from stdin
                text = self.fin.read()
                util.writefile(name, text)
            else:
                with perftrace.trace("Editor"):
                    self.system(
                        f"{editor} {util.shellquote(name)}",
                        environ=environ,
                        onerr=error.Abort,
                        errprefix=_("edit failed"),
                        blockedtag="editor",
                    )

            f = open(name, r"rb")
            t = util.fromnativeeol(decodeutf8(f.read()))
            f.close()
        finally:
            if rdir is None:
                # If repo path is not provided, the file lives in system tmp,
                # remove it immediately.
                os.unlink(name)
            else:
                # If editing in .hg/edit-tmp, remove files older than 2 weeks.
                util.gcdir(rdir, 24 * 3600 * 14)
        return t

    def system(
        self,
        cmd,
        environ=None,
        cwd=None,
        onerr=None,
        errprefix=None,
        blockedtag=None,
        suspendprogress=True,
    ):
        """execute shell command with appropriate output stream. command
        output will be redirected if fout is not stdout.

        if command fails and onerr is None, return status, else raise onerr
        object as exception.
        """
        if blockedtag is None:
            blockedtag = "unknown_system"
        out = self.fout
        if any(s[1] for s in self._bufferstates):
            out = self
        if suspendprogress:
            suspend = progress.suspend
        else:
            suspend = util.nullcontextmanager
        with self.timeblockedsection(blockedtag), suspend(), util.traced(
            blockedtag, cat="blocked"
        ):
            rc = self._runsystem(cmd, environ=environ, cwd=cwd, out=out)
        if rc and onerr:
            errmsg = "%s %s" % (
                os.path.basename(cmd.split(None, 1)[0]),
                util.explainexit(rc)[0],
            )
            if errprefix:
                errmsg = "%s: %s" % (errprefix, errmsg)
            raise onerr(errmsg)
        return rc

    def _runsystem(self, cmd, environ, cwd, out):
        """actually execute the given shell command (can be overridden by
        extensions like chg)"""
        return util.rawsystem(cmd, environ=environ, cwd=cwd, out=out)

    def traceback(self, exc=None, force=False):
        """print exception traceback if traceback printing enabled or forced.
        only to call in exception handler. returns true if traceback
        printed."""
        if self.tracebackflag or force:
            if exc is None:
                exc = sys.exc_info()
            fancy = self.configbool("ui", "fancy-traceback")
            cause = getattr(exc[1], "cause", None)

            # Collapse traceback to make it easier for tests.
            collapse = self.configbool("devel", "collapse-traceback")

            if cause is not None:
                if collapse:
                    causetb = ["  # collapsed by devel.collapse-traceback"]
                    exctb = []
                else:
                    if fancy:
                        causetb = util.smarttraceback(cause[2])
                    else:
                        causetb = traceback.format_tb(cause[2])
                    exctb = traceback.format_tb(exc[2])
                exconly = traceback.format_exception_only(cause[0], cause[1])

                # exclude frame where 'exc' was chained and rethrown from exctb
                self.write_err(
                    "Traceback (most recent call last):\n",
                    "".join(exctb[:-1]),
                    "".join(causetb),
                    "".join(exconly),
                )
            else:
                if collapse:
                    exconly = traceback.format_exception_only(exc[0], exc[1])
                    data = (
                        "Traceback (most recent call last):\n"
                        "  # collapsed by devel.collapse-traceback\n"
                    ) + "".join(exconly)
                else:
                    if fancy:
                        data = util.smartformatexc(exc)
                    else:
                        output = traceback.format_exception(exc[0], exc[1], exc[2])
                        data = r"".join(output)
                self.write_err(data)
        return self.tracebackflag or force

    def geteditor(self):
        """return editor to use"""
        if pycompat.sysplatform == "plan9":
            # vi is the MIPS instruction simulator on Plan 9. We
            # instead default to E to plumb commit messages to
            # avoid confusion.
            defaulteditor = "E"
        elif pycompat.sysplatform == "win32":
            defaulteditor = "notepad.exe"
        else:

            defaulteditor = "vi"
        return (
            encoding.environ.get("HGEDITOR")
            or self.config(
                "ui",
                "editor",
            )
            or defaulteditor
        )

    def progress(self):
        """deprecated method for displaying progress"""
        raise NotImplementedError()

    def log(self, service, *msg, **opts):
        """hook for logging facility extensions

        service should be a readily-identifiable subsystem, which will
        allow filtering.

        *msg should be a newline-terminated format string to log, and
        then any values to %-format into that format string.

        **opts is a dict of additional key-value pairs to log.

        This method is being slowly deprecated. Use 'blackbox.log' instead.
        """
        origmsg = msg
        if not msg:
            msg = ""
        elif len(msg) > 1:
            try:
                msg = msg[0] % msg[1:]
            except TypeError:
                # "TypeError: not enough arguments for format string"
                # Fallback to just concat the strings. Ideally this fallback is
                # not necessary.
                msg = " ".join(msg)
        else:
            msg = msg[0]
        try:
            blackbox.log({"legacy_log": {"service": service, "msg": msg, "opts": opts}})
        except UnicodeDecodeError:
            pass

        self._logsample(service, *origmsg, **opts)

    def deprecate(
        self, name, message, maxlevel=deprecationlevel.Log, startstr=None, endstr=None
    ):
        """marks a code path as deprecated

        The default behavior is to simply log the usage of the deprecated path,
        but `maxlevel` can be used to specify stricter deprecation strategies.

        If `start` and `end` are provided, the deprecation level will be slowly
        increased over the course of the `start` and `end` time, reaching the
        specified `maxlevel` at the end time.
        """
        level = maxlevel
        if startstr is not None and endstr is not None:
            now = time.time()
            start = util.parsedate(startstr)[0]
            end = util.parsedate(endstr)[0]
            # Linearly interpolate to get the current level
            percent = float(now - start) / float(end - start)
            level = max(0, min(int(percent * maxlevel), maxlevel))

        caller = util.caller()
        self.log(
            "deprecated",
            message,
            feature=name,
            level=int(level),
            version=util.version(),
            caller=caller,
        )

        bypassed = self.configbool("deprecated", "bypass-%s" % name)
        if level == deprecationlevel.Block:
            raise error.DeprecatedError(
                _("feature '%s' is disabled: %s") % (name, message)
            )
        elif level == deprecationlevel.Optin and not bypassed:
            hint = (
                _(
                    "set config `deprecated.bypass-%s=True` to temporarily bypass this block"
                )
                % name
            )
            if endstr is not None and maxlevel == deprecationlevel.Block:
                hint = _(
                    "set config `deprecated.bypass-%s=True` to bypass this block, but note the feature will be completely disabled on %s"
                ) % (name, endstr)
            raise error.DeprecatedError(
                _("feature '%s' is disabled: %s") % (name, message), hint=hint
            )
        elif level >= deprecationlevel.Slow and not bypassed:
            self.warn(
                _(
                    "warning: sleeping for 2 seconds because feature '%s' is deprecated: %s\n"
                )
                % (name, message)
            )
            self.warn(
                _(
                    "note: the feature will be completely disabled soon, so please migrate off\n"
                )
            )
            time.sleep(2)
        elif level >= deprecationlevel.Warn:
            self.warn(_("warning: feature '%s' is deprecated: %s\n") % (name, message))
            self.warn(
                _(
                    "note: the feature will be completely disabled soon, so please migrate off\n"
                )
            )
        else:
            self.develwarn(_("feature '%s' is deprecated: %s\n") % (name, message))

    def _computesamplingfilters(self):
        filtermap = {}
        for k in self.configitems("sampling"):
            if not k[0].startswith("key."):
                continue  # not a key
            filtermap[k[0][len("key.") :]] = k[1]
        return filtermap

    def _getcandidatelocation(self):
        def _parentfolderexists(f):
            return f is not None and os.path.exists(
                os.path.dirname(os.path.normpath(f))
            )

        for candidatelocation in (
            encoding.environ.get("SCM_SAMPLING_FILEPATH", None),
            self.config("sampling", "filepath"),
        ):
            if _parentfolderexists(candidatelocation):
                return candidatelocation
        return None

    def _logsample(self, event, *msg, **opts):
        """Redirect filtered log event to a sampling file
        The configuration looks like:
        [sampling]
        filepath = path/to/file
        key.eventname = value
        key.eventname2 = value2

        If an event name appears in the config, it is logged to the
        samplingfile augmented with value stored as ref.

        Example:
        [sampling]
        filepath = path/to/file
        key.perfstatus = perf_status

        Assuming that we call:
        ui.log('perfstatus', t=3)
        ui.log('perfcommit', t=3)
        ui.log('perfstatus', t=42)

        Then we will log in path/to/file, two JSON strings separated by \0
        one for each perfstatus, like:
        {"event":"perfstatus",
         "ref":"perf_status",
         "msg":"",
         "opts":{"t":3}}\0
        {"event":"perfstatus",
         "ref":"perf_status",
         "msg":"",
         "opts":{"t":42}}\0

        We will also log any given environmental vars to the env_vars log,
        if configured::

          [sampling]
          env_vars = PATH,SHELL
        """
        if not util.safehasattr(self, "samplingfilters"):
            self.samplingfilters = self._computesamplingfilters()
        if event not in self.samplingfilters:
            return

        # special case: remove less interesting blocked fields starting
        # with "unknown_" or "alias_".
        if event == "measuredtimes":
            opts = {
                k: v
                for k, v in opts.items()
                if (not k.startswith("alias_") and not k.startswith("unknown_"))
            }

        ref = self.samplingfilters[event]
        script = self._getcandidatelocation()
        if script:
            debug = self.configbool("sampling", "debug")
            try:
                opts["metrics_type"] = event
                if msg and event != "metrics":
                    # do not keep message for "metrics", which only wants
                    # to log key/value dict.
                    if len(msg) == 1:
                        # don't try to format if there is only one item.
                        opts["msg"] = msg[0]
                    else:
                        # ui.log treats msg as a format string + format args.
                        try:
                            opts["msg"] = msg[0] % msg[1:]
                        except TypeError:
                            # formatting failed - just log each item of the
                            # message separately.
                            opts["msg"] = " ".join(msg)
                with open(script, "a") as outfile:
                    outfile.write(
                        pycompat.toutf8lossy(
                            json.dumps({"data": opts, "category": ref})
                        )
                    )
                    outfile.write("\0")
                if debug:
                    self.write_err(
                        "%s\n"
                        % pycompat.toutf8lossy(
                            json.dumps({"data": opts, "category": ref})
                        )
                    )
            except EnvironmentError:
                pass

    def label(self, msg, label, usebytes=False):
        """style msg based on supplied label

        If some color mode is enabled, this will add the necessary control
        characters to apply such color. In addition, 'debug' color mode adds
        markup showing which label affects a piece of text.

        ui.write(s, 'label') is equivalent to
        ui.write(ui.label(s, 'label')).
        """
        if self._colormode is not None:
            return color.colorlabel(self, msg, label, usebytes=usebytes)
        return msg

    def develwarn(self, msg, stacklevel=1, config=None):
        """issue a developer warning message

        Use 'stacklevel' to report the offender some layers further up in the
        stack.
        """
        if not self.configbool("devel", "all-warnings"):
            if config is None or not self.configbool("devel", config):
                return
        msg = "devel-warn: " + msg
        stacklevel += 1  # get in develwarn
        if self.tracebackflag:
            util.debugstacktrace(msg, stacklevel, self.ferr, self.fout)
            self.log(
                "develwarn",
                "%s at:\n%s" % (msg, "".join(util.getstackframes(stacklevel))),
            )
        else:
            curframe = inspect.currentframe()
            calframe = inspect.getouterframes(curframe, 2)
            self.write_err("%s at: %s:%s (%s)\n" % ((msg,) + calframe[stacklevel][1:4]))
            self.log(
                "develwarn", "%s at: %s:%s (%s)\n", msg, *calframe[stacklevel][1:4]
            )
            curframe = calframe = None  # avoid cycles

    def deprecwarn(self, msg, version):
        """issue a deprecation warning

        - msg: message explaining what is deprecated and how to upgrade,
        - version: last version where the API will be supported,
        """
        if not (
            self.configbool("devel", "all-warnings")
            or self.configbool("devel", "deprec-warn")
        ):
            return
        msg += (
            "\n(compatibility will be dropped after Mercurial-%s," " update your code.)"
        ) % version
        self.develwarn(msg, stacklevel=2, config="deprec-warn")

    def exportableenviron(self):
        """The environment variables that are safe to export."""
        return self._exportableenviron

    @contextlib.contextmanager
    def configoverride(self, overrides, source=""):
        """Context manager for temporary config overrides
        `overrides` must be a dict of the following structure:
        {(section, name) : value}"""
        with self._uiconfig.configoverride(overrides, source):
            yield

    def uiconfig(self):
        return self._uiconfig

    @property
    def _rcfg(self):
        return self._uiconfig._rcfg

    @property
    def quiet(self):
        return self._uiconfig.quiet

    @quiet.setter
    def quiet(self, value):
        self._uiconfig.quiet = value

    @property
    def verbose(self):
        return self._uiconfig.verbose

    @verbose.setter
    def verbose(self, value):
        self._uiconfig.verbose = value

    @property
    def debugflag(self):
        return self._uiconfig.debugflag

    @property
    def tracebackflag(self):
        return self._uiconfig.tracebackflag

    @property
    def logmeasuredtimes(self):
        return self._uiconfig.logmeasuredtimes


def _normalizepath(rawloc: str) -> str:
    rawloc = rawloc.split("?", 1)[0]
    if rawloc.startswith("file:"):
        rawloc = rawloc[5:]
    if pycompat.iswindows:
        rawloc = rawloc.replace("\\", "/")
    if os.path.sep != "/":
        rawloc = rawloc.replace(":///", ":")
    rawloc = rawloc.replace("://", ":")
    # remove trailing slash
    rawloc = rawloc.rstrip("/")
    return rawloc


class paths(util.sortdict):
    """Represents a collection of paths and their configs.

    Data is initially derived from ui instances and the config files they have
    loaded.
    """

    def __init__(self, ui):
        super(paths, self).__init__(self)
        self._uiconfig = ui.uiconfig()

        for name, loc in ui.configitems("paths", ignoresub=True):
            # No location is the same as not existing.
            if not loc:
                continue
            loc, sub = ui.configsuboptions("paths", name)
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

        # Normalize the name according to remotenames.rename.*
        # NOTE: Consider just rename "default" to "remote" in tests and get
        # rid of the remotenames.rename.* configs.
        for k, v in self._uiconfig.configitems("remotenames"):
            if v == name and k.startswith("rename."):
                name = k[len("rename.") :]

        try:
            return self[name]
        except KeyError:
            # Try to resolve as a local path or URI.
            try:
                # We don't pass sub-options in, so no need to pass ui instance.
                return path(None, None, rawloc=name)
            except ValueError:
                raise error.RepoError(_("repository %s does not exist") % name)

    def getname(self, rawloc, forremotenames=False):
        """Return name from a raw location.

        If this function is about to return $name, and
        'remotenames.rename.$name' config exists, return the value of that
        config instead.

        If 'forremotenames' is True, normalize 'default-push' to 'default'.
        This is only used by 'bookmarks.remotenameforurl' so we never write
        'default-push' as a remote name. If you're setting this flag, consider
        using 'bookmarks.remotenameforurl' instead.

        Return `None` if path is unknown.
        """

        rawloc = _normalizepath(rawloc)
        result = None
        for name, path in self.items():
            if _normalizepath(path.rawloc) == rawloc:
                result = name
                break

        # XXX: Remove this normalization if Mononoke is rolled out to all.
        if result in {"infinitepush", "infinitepushbookmark"}:
            result = "default"

        # Do not use 'default-push' as a remote name. Normalize it to
        # 'default'.
        if forremotenames and result == "default-push":
            result = "default"

        if result:
            renamed = self._uiconfig.config("remotenames", "rename.%s" % result)
            if renamed:
                result = renamed
        return result


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


@pathsuboption("pushurl", "pushloc")
def pushurlpathoption(ui, path, value):
    u = util.url(value)
    # Actually require a URL.
    if not u.scheme:
        ui.warn(_("(paths.%s:pushurl not a URL; ignoring)\n") % path.name)
        return None

    # Don't support the #foo syntax in the push URL to declare branch to
    # push.
    if u.fragment:
        ui.warn(
            _('("#fragment" in paths.%s:pushurl not supported; ' "ignoring)\n")
            % path.name
        )
        u.fragment = None

    return str(u)


@pathsuboption("pushrev", "pushrev")
def pushrevpathoption(ui, path, value):
    return value


def _normalize_rawloc(rawloc: str) -> str:
    """Normalize a raw location:

    - If rawloc is a local path backed by an eager repo, return "eager:rawloc".
      The "eager:" scheme helps various places like `repo.edenapi` correctly
      realize the remote peer is an eager repo.
    """
    if os.path.isabs(rawloc):
        try:
            ident = identity.sniffdir(rawloc)
            if not ident:
                return rawloc

            with open(os.path.join(rawloc, ident.dotdir(), "store", "requires")) as f:
                from .eagerepo import EAGEREPO_REQUIREMENT

                if EAGEREPO_REQUIREMENT in f.read().split():
                    return f"eager:{rawloc}"
        except IOError:
            pass
    return rawloc


class path(object):
    """Represents an individual path and its configuration."""

    _all_dotdirs = [ident.dotdir() for ident in bindings.identity.all()]

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
            raise ValueError("rawloc must be defined")

        rawloc = _normalize_rawloc(rawloc)

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
        self.loc = "%s" % u

        # When given a raw location but not a symbolic name, validate the
        # location is valid.
        if not name and not u.scheme and not self._isvalidlocalpath(self.loc):
            raise ValueError(
                "location is not a URL or path to a local " "repo: %s" % rawloc
            )

        suboptions = suboptions or {}

        # Now process the sub-options. If a sub-option is registered, its
        # attribute will always be present. The value will be None if there
        # was no valid sub-option.
        for suboption, (attr, func) in pycompat.iteritems(_pathsuboptions):
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
        # objects/: potentially a bare git repo
        return any(
            os.path.isdir(os.path.join(path, name))
            for name in (*self.__class__._all_dotdirs, ".git", "objects")
        )

    @property
    def suboptions(self):
        """Return sub-options and their values for this path.

        This is intended to be used for presentation purposes.
        """
        d = {}
        for subopt, (attr, _func) in pycompat.iteritems(_pathsuboptions):
            value = getattr(self, attr)
            if value is not None:
                d[subopt] = value
        return d
