# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import contextlib
import os
import random
import time
from typing import List, Optional, Tuple

from bindings import configloader

from . import configitems, error, pycompat, util
from .encoding import unifromlocal, unitolocal
from .i18n import _


# unique object used to detect no default value has been provided when
# retrieving configuration value.
_unset = object()


def optional(func, s):
    if s is None:
        return None
    else:
        return func(s)


class uiconfig(object):
    """Config portion of the ui object"""

    def __init__(self, src=None, rcfg=None):
        """Create a fresh new uiconfig object.

        Or copy from an existing uiconfig object.
        """
        # Cached values. Will be rewritten by "fixconfig".
        self.quiet = self.verbose = self.debugflag = self.tracebackflag = False
        self.logmeasuredtimes = False

        if src:
            self._rcfg = src._rcfg.clone()
            self._unserializable = src._unserializable.copy()
            self._pinnedconfigs = src._pinnedconfigs.copy()
            self._knownconfig = src._knownconfig
        else:
            self._rcfg = rcfg or configloader.config()
            # map from IDs to unserializable Python objects.
            self._unserializable = {}
            # config "pinned" that cannot be loaded from files.
            # ex. --config flags
            self._pinnedconfigs = set()
            self._knownconfig = configitems.coreitems

        self.fixconfig()

    @classmethod
    def load(cls, ui, repopath):
        """Create a uiconfig and load global and user configs"""
        u = cls()
        try:
            # repopath should be the non-shared root directory
            rcfg = configloader.config.load(repopath or None)
        except Exception as ex:
            raise error.ParseError(str(ex))

        u._rcfg = rcfg
        ui._uiconfig = u

        root = os.path.expanduser("~")
        u.fixconfig(root=repopath or root)

    def reload(self, ui, repopath):
        # The actual config expects the non-shared root directory.
        self._rcfg.reload(repopath, list(self._pinnedconfigs))

        # fixconfig expects the non-shard repo root, without the .hg.
        self.fixconfig(root=repopath)

    def copy(self):
        return self.__class__(self)

    def readconfig(
        self,
        filename,
        root=None,
        trust=False,
        sections=None,
        remap=None,
        source="ui.readconfig",
    ):
        cfg = self._rcfg
        errors = cfg.readpath(
            filename,
            source,
            sections,
            remap and remap.items(),
            list(self._pinnedconfigs),
        )
        if errors:
            raise error.ParseError("\n\n".join(errors))

        if root is None:
            root = os.path.expanduser("~")

        self.fixconfig(root=root)

    def fixconfig(self, root=None, section=None):
        if section in (None, "paths"):
            # expand vars and ~
            # translate paths relative to root (or home) into absolute paths
            c = self._rcfg
            for n in c.names("paths"):
                if ":" in n:
                    continue
                p = origp = c.get("paths", n)
                if not p:
                    continue
                if "%%" in p:
                    s = self.configsource("paths", n) or "none"
                    self.warn(
                        _("(deprecated '%%' in path %s=%s from %s)\n") % (n, p, s)
                    )
                    p = p.replace("%%", "%")
                p = util.expandpath(p)
                if not util.hasscheme(p) and not os.path.isabs(p):
                    if not root:
                        # Don't expand relative path if we don't know the repo root.
                        continue
                    p = os.path.normpath(os.path.join(root, p))
                if origp != p:
                    c.set("paths", n, p, "ui.fixconfig")

        if section in (None, "ui"):
            # update ui options
            self.debugflag = self.configbool("ui", "debug")
            self.verbose = self.debugflag or self.configbool("ui", "verbose")
            self.quiet = not self.debugflag and self.configbool("ui", "quiet")
            if self.verbose and self.quiet:
                self.quiet = self.verbose = False
            self.tracebackflag = self.configbool("ui", "traceback")
            self.logmeasuredtimes = self.configbool("ui", "logmeasuredtimes")

    def setconfig(self, section, name, value, source=""):
        if isinstance(value, (str, int, float, bool)):
            value = str(value)
        elif util.safehasattr(value, "__iter__"):

            def escape(v):
                if '"' in v:
                    v = v.replace('"', '\\"')
                return '"%s"' % v

            value = ",".join(escape(v) for v in value)
        elif value is None:
            pass
        else:
            # XXX Sad - Some code depends on setconfig a Python object.
            # That cannot be represented in the Rust config object. So
            # we translate them here in a very hacky way.
            # TODO remove those users and make this a ProgrammingError.
            replacement = "@%x" % id(value)
            self._unserializable[replacement] = value
            value = replacement

        self._pinnedconfigs.add((section, name))
        self._rcfg.set(section, name, value, source or "ui.setconfig")
        self.fixconfig(section=section)

    def configtostring(self):
        return self._rcfg.tostring()

    def configsource(self, section, name):
        sources = self._rcfg.sources(section, name)
        if sources:
            # Skip "ui.fixconfig" sources
            for value, filesource, strsource in reversed(sources):
                if strsource == "ui.fixconfig":
                    continue
                if filesource:
                    path, _start, _end, line = filesource
                    return "%s:%s" % (path, line)
                else:
                    return strsource
        return ""

    def config(self, section, name, default=_unset):
        """return the plain string version of a config"""
        value = self._config(section, name, default=default)
        if value is _unset:
            return None
        return value

    def _config(self, section, name, default=_unset):
        value = itemdefault = default
        item = self._knownconfig.get(section, {}).get(name)
        alternates = [(section, name)]

        if item is not None:
            alternates.extend(item.alias)
            if callable(item.default):
                itemdefault = item.default()
            else:
                itemdefault = item.default
        # fbonly: disabled in a hotfix because it's so expensive to fix
        elif False:
            msg = "accessing unregistered config item: '%s.%s'"
            msg %= (section, name)
            self.develwarn(msg, 2, "warn-config-unknown")

        if default is _unset:
            if item is None:
                value = default
            elif item.default is configitems.dynamicdefault:
                value = None
                msg = "config item requires an explicit default value: '%s.%s'"
                msg %= (section, name)
                self.develwarn(msg, 2, "warn-config-default")
            else:
                value = itemdefault
        elif (
            item is not None
            and item.default is not configitems.dynamicdefault
            and default != itemdefault
        ):
            msg = (
                "specifying a mismatched default value for a registered "
                "config item: '%s.%s' '%s'"
            )
            msg %= (section, name, default)
            self.develwarn(msg, 2, "warn-config-default")

        for s, n in alternates:
            candidate = self._rcfg.get(s, n)
            if candidate is not None:
                value = candidate
                value = self._unserializable.get(value, value)
                section = s
                name = n
                break

        return value

    def configsuboptions(self, section, name, default=_unset):
        """Get a config option and all sub-options.

        Some config options have sub-options that are declared with the
        format "key:opt = value". This method is used to return the main
        option and all its declared sub-options.

        Returns a 2-tuple of ``(option, sub-options)``, where `sub-options``
        is a dict of defined sub-options where keys and values are strings.
        """
        main = self.config(section, name, default)
        cfg = self._rcfg
        sub = {}
        prefix = "%s:" % name
        for k in cfg.names(section):
            if k.startswith(prefix):
                v = cfg.get(section, k)
                v = self._unserializable.get(v, v)
                sub[k[len(prefix) :]] = v
        return main, sub

    def configpath(self, section, name, default=_unset):
        "get a path config item, expanded relative to repo root or config file"
        v = self.config(section, name, default)
        if v is None:
            return None
        if not os.path.isabs(v) or "://" not in v:
            src = self.configsource(section, name)
            if ":" in src:
                base = os.path.dirname(src.rsplit(":")[0])
                v = os.path.join(base, os.path.expanduser(v))
        return v

    def configbool(self, section, name, default=_unset):
        """parse a configuration element as a boolean

        >>> u = uiconfig(); s = 'foo'
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

        v = self._config(section, name, default)
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
            raise error.ConfigError(
                _("%s.%s is not a boolean ('%s')") % (section, name, v)
            )
        return b

    def configwith(self, convert, section, name, default=_unset, desc=None):
        """parse a configuration element with a conversion function

        >>> u = uiconfig(); s = 'foo'
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

        v = self.config(section, name, default)
        if v is None:
            return v  # do not attempt to convert None
        try:
            return convert(v)
        except (ValueError, error.ParseError):
            if desc is None:
                desc = convert.__name__
            raise error.ConfigError(
                _("%s.%s is not a valid %s ('%s')") % (section, name, desc, v)
            )

    def configint(self, section, name, default=_unset):
        """parse a configuration element as an integer

        >>> u = uiconfig(); s = 'foo'
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

        return self.configwith(int, section, name, default, "integer")

    def configbytes(self, section, name, default=_unset):
        """parse a configuration element as a quantity in bytes

        Units can be specified as b (bytes), k or kb (kilobytes), m or
        mb (megabytes), g or gb (gigabytes).

        >>> u = uiconfig(); s = 'foo'
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

        value = self._config(section, name, default)
        if value is _unset:
            if default is _unset:
                default = 0
            value = default
        if not isinstance(value, str):
            return value
        try:
            return util.sizetoint(value)
        except error.ParseError:
            raise error.ConfigError(
                _("%s.%s is not a byte quantity ('%s')") % (section, name, value)
            )

    def configlist(self, section, name, default=_unset):
        """parse a configuration element as a list of comma/space separated
        strings

        >>> u = uiconfig(); s = 'foo'
        >>> u.setconfig(s, 'list1', 'this,is "a small" ,test')
        >>> u.configlist(s, 'list1')
        ['this', 'is', 'a small', 'test']
        >>> u.setconfig(s, 'list2', 'this, is "a small" , test ')
        >>> u.configlist(s, 'list2')
        ['this', 'is', 'a small', 'test']
        """
        # default is not always a list
        v = self.configwith(parselist, section, name, default, "list")
        if isinstance(v, str):
            return parselist(v)
        elif v is None:
            return []
        return v

    def configdate(self, section, name, default=_unset):
        """parse a configuration element as a tuple of ints

        >>> u = uiconfig(); s = 'foo'
        >>> u.setconfig(s, 'date', '0 0')
        >>> u.configdate(s, 'date')
        (0, 0)
        """
        if self.config(section, name, default):
            return self.configwith(util.parsedate, section, name, default, "date")
        if default is _unset:
            return None
        return default

    def hasconfig(self, section, name):
        return self._rcfg.get(section, name) is not None

    def has_section(self, section):
        """tell whether section exists in config."""
        return section in self._rcfg.sections()

    def configsections(self):
        return self._rcfg.sections()

    def configitems(self, section, ignoresub=False):
        cfg = self._rcfg
        items = []
        for name in cfg.names(section):
            value = cfg.get(section, name)
            value = self._unserializable.get(value, value)
            if value is not None:
                if not ignoresub or ":" not in name:
                    items.append((name, value))
        return items

    def walkconfig(self):
        cfg = self._rcfg
        for section in sorted(cfg.sections()):
            for name in cfg.names(section):
                value = cfg.get(section, name)
                value = self._unserializable.get(value, value)
                if value is not None:
                    yield section, name, value

    @contextlib.contextmanager
    def configoverride(self, overrides, source=""):
        """Context manager for temporary config overrides
        `overrides` must be a dict of the following structure:
        {(section, name) : value}"""
        backup = self._rcfg.clone()
        unserializablebackup = dict(self._unserializable)
        pinnedbackup = set(self._pinnedconfigs)
        try:
            for (section, name), value in overrides.items():
                self.setconfig(section, name, value, source)
            yield
        finally:
            self._rcfg = backup
            self._unserializable = unserializablebackup
            self._pinnedconfigs = pinnedbackup

            # just restoring ui.quiet config to the previous value is not enough
            # as it does not update ui.quiet class member
            if ("ui", "quiet") in overrides:
                self.fixconfig(section="ui")

    def setclioverrides(self, cliconfigs, cliconfigfiles):
        # --config takes prescendence over --configfile, so process
        # --configfile first then --config second.
        for configfile in cliconfigfiles:
            tempconfig = uiconfig()
            tempconfig.readconfig(configfile)
            # Set the configfile values one-by-one so they get put in the internal
            # _pinnedconfigs list and don't get overwritten in the future.
            for section, name, value in tempconfig.walkconfig():
                self.setconfig(section, name, value, configfile)

        for cfg in cliconfigs:
            try:
                name, value = [cfgelem.strip() for cfgelem in cfg.split("=", 1)]
                section, name = name.split(".", 1)
                if not section or not name:
                    raise IndexError
                self.setconfig(section, name, value, "--config")
            except (IndexError, ValueError):
                raise error.Abort(
                    _(
                        "malformed --config option: %r "
                        "(use --config section.name=value)"
                    )
                    % cfg
                )

        if cliconfigfiles:
            self.setconfig("_configs", "configfiles", cliconfigfiles)

    def develwarn(self, msg, stacklevel=1, config=None):
        # FIXME: Do something here?
        pass


def parselist(value):
    if isinstance(value, str):
        return [unitolocal(v) for v in configloader.parselist(unifromlocal(value))]
    else:
        return value


# TODO Call this from somewhere
def logages(ui, configpath, cachepath):
    kwargs = dict()
    for path, name in [
        (cachepath, "dynamicconfig_remote_age"),
        # We do the config age last, so we can return the mtime
        (configpath, "dynamicconfig_age"),
    ]:
        # Default to the unix epoch as the mtime
        mtime = 0
        if os.path.exists(path):
            mtime = os.lstat(path).st_mtime

        age = time.time() - mtime
        # Round it so we get better compression upstream.
        age = age - (age % 10)

        kwargs[name] = age

    ui.log("dynamicconfig_age", **kwargs)

    return mtime
