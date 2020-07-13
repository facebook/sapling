# Portions Copyright (c) Facebook, Inc. and its affiliates.
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
import re
import subprocess
import time
from typing import List, Optional, Tuple

from bindings import configparser, dynamicconfig

from ..hgext.extutil import runbgcommand
from . import configitems, encoding, error, pycompat, util
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


class localrcfg(object):
    """Wrapper to the Rust config object that does proper encoding translation.

    Note: This is no longer needed once we migrate to Python 3.
    """

    def __init__(self, rcfg):
        self._rcfg = rcfg

    def get(self, section, name):
        # type: (str, str) -> Optional[str]
        usection = unifromlocal(section)
        uname = unifromlocal(name)
        uvalue = self._rcfg.get(usection, uname)
        return optional(unitolocal, uvalue)

    def sources(self, section, name):
        # type: (str, str) -> List[Tuple[Optional[str], Optional[Tuple[str, int, int, int]], str]]
        result = []
        for (uvalue, info, usource) in self._rcfg.sources(section, name):
            value = optional(unitolocal, uvalue)
            source = optional(unitolocal, usource)
            result.append((value, info, source))
        return result

    def set(self, section, name, value, source):
        # type: (str, str, Optional[str], str) -> None
        usection = unifromlocal(section)
        uname = unifromlocal(name)
        uvalue = optional(unifromlocal, value)
        usource = optional(unifromlocal, source)
        self._rcfg.set(usection, uname, uvalue, usource)

    def sections(self):
        # type: () -> List[str]
        return [unitolocal(s) for s in self._rcfg.sections()]

    def names(self, section):
        # type: (str) -> List[str]
        usection = unifromlocal(section)
        return [unitolocal(s) for s in self._rcfg.names(usection)]

    def clone(self):
        # type: () -> localrcfg
        return localrcfg(self._rcfg.clone())

    def __getattr__(self, name):
        return getattr(self._rcfg, name)


class uiconfig(object):
    """Config portion of the ui object"""

    def __init__(self, src=None):
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

            self.fixconfig()
        else:
            self._rcfg = localrcfg(configparser.config())
            # map from IDs to unserializable Python objects.
            self._unserializable = {}
            # config "pinned" that cannot be loaded from files.
            # ex. --config flags
            self._pinnedconfigs = set()
            self._knownconfig = configitems.coreitems

    @classmethod
    def load(cls):
        """Create a uiconfig and load global and user configs"""
        u = cls()
        rcfg, errors = configparser.config.load()
        u._rcfg = localrcfg(rcfg)
        if errors:
            raise error.ParseError("\n\n".join(errors))
        root = os.path.expanduser("~")
        u.fixconfig(root=root)
        return u

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
            root = root or pycompat.getcwd()
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

    def configsource(self, section, name, untrusted=False):
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

    def config(self, section, name, default=_unset, untrusted=False):
        """return the plain string version of a config"""
        value = self._config(section, name, default=default, untrusted=untrusted)
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

    def configsuboptions(self, section, name, default=_unset, untrusted=False):
        """Get a config option and all sub-options.

        Some config options have sub-options that are declared with the
        format "key:opt = value". This method is used to return the main
        option and all its declared sub-options.

        Returns a 2-tuple of ``(option, sub-options)``, where `sub-options``
        is a dict of defined sub-options where keys and values are strings.
        """
        main = self.config(section, name, default, untrusted=untrusted)
        cfg = self._rcfg
        sub = {}
        prefix = "%s:" % name
        for k in cfg.names(section):
            if k.startswith(prefix):
                v = cfg.get(section, k)
                v = self._unserializable.get(v, v)
                sub[k[len(prefix) :]] = v
        return main, sub

    def configpath(self, section, name, default=_unset, untrusted=False):
        "get a path config item, expanded relative to repo root or config file"
        v = self.config(section, name, default, untrusted)
        if v is None:
            return None
        if not os.path.isabs(v) or "://" not in v:
            src = self.configsource(section, name, untrusted)
            if ":" in src:
                base = os.path.dirname(src.rsplit(":")[0])
                v = os.path.join(base, os.path.expanduser(v))
        return v

    def configbool(self, section, name, default=_unset, untrusted=False):
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
        >>> u.configbool(s, 'invalid')
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
            raise error.ConfigError(
                _("%s.%s is not a boolean ('%s')") % (section, name, v)
            )
        return b

    def configwith(
        self, convert, section, name, default=_unset, desc=None, untrusted=False
    ):
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
        >>> u.configwith(float, s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid float ('somevalue')
        >>> u.configwith(float, s, 'invalid', desc='womble')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid womble ('somevalue')
        """

        v = self.config(section, name, default, untrusted)
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

    def configint(self, section, name, default=_unset, untrusted=False):
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
        >>> u.configint(s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a valid integer ('somevalue')
        """

        return self.configwith(int, section, name, default, "integer", untrusted)

    def configbytes(self, section, name, default=_unset, untrusted=False):
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
        >>> u.configbytes(s, 'invalid')
        Traceback (most recent call last):
            ...
        ConfigError: foo.invalid is not a byte quantity ('somevalue')
        """

        value = self._config(section, name, default, untrusted)
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

    def configlist(self, section, name, default=_unset, untrusted=False):
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
        v = self.configwith(parselist, section, name, default, "list", untrusted)
        if isinstance(v, str):
            return parselist(v)
        elif v is None:
            return []
        return v

    def configdate(self, section, name, default=_unset, untrusted=False):
        """parse a configuration element as a tuple of ints

        >>> u = uiconfig(); s = 'foo'
        >>> u.setconfig(s, 'date', '0 0')
        >>> u.configdate(s, 'date')
        (0, 0)
        """
        if self.config(section, name, default, untrusted):
            return self.configwith(
                util.parsedate, section, name, default, "date", untrusted
            )
        if default is _unset:
            return None
        return default

    def hasconfig(self, section, name, untrusted=False):
        return self._rcfg.get(section, name) is not None

    def has_section(self, section, untrusted=False):
        """tell whether section exists in config."""
        return section in self._rcfg.sections()

    def configsections(self):
        return self._rcfg.sections()

    def configitems(self, section, untrusted=False, ignoresub=False):
        cfg = self._rcfg
        items = []
        for name in cfg.names(section):
            value = cfg.get(section, name)
            value = self._unserializable.get(value, value)
            if value is not None:
                if not ignoresub or ":" not in name:
                    items.append((name, value))
        return items

    def walkconfig(self, untrusted=False):
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

    def develwarn(self, msg, stacklevel=1, config=None):
        # FIXME: Do something here?
        pass


def parselist(value):
    if isinstance(value, str):
        return [unitolocal(v) for v in configparser.parselist(unifromlocal(value))]
    else:
        return value


def loaddynamicconfig(ui, path):
    if ui.configbool("configs", "loaddynamicconfig"):
        sharedpathfile = os.path.join(path, "sharedpath")
        if os.path.exists(sharedpathfile):
            with open(sharedpathfile, "rb") as f:
                path = pycompat.decodeutf8(f.read())

        hgrcdyn = os.path.join(path, "hgrc.dynamic")

        # Check the version of the existing generated config. If it doesn't
        # match the current version, regenerate it immediately.
        try:
            with open(hgrcdyn, "rb") as f:
                content = pycompat.decodeutf8(f.read())
            matches = re.search("^# version=(.*)$", content, re.MULTILINE)
            version = matches.group(1) if matches else None
        except IOError:
            version = None

        if version is None or version != util.version():
            try:
                ui.debug(
                    "synchronously generating dynamic config - new version %s, old version %s\n"
                    % (util.version(), version)
                )
                reponame = ui.config("remotefilelog", "reponame") or ""
                generatedynamicconfig(ui, reponame, path)
            except Exception as ex:
                # TODO: Eventually this should throw an exception, once we're
                # confident it's reliable.
                ui.log(
                    "exceptions",
                    "unable to generate dynamicconfig",
                    exception_type="DynamicconfigGeneration",
                    exception_msg="unable to generate dynamicconfig: %s" % str(ex),
                )

        ui.readconfig(hgrcdyn, path)
        mtime = logages(ui, hgrcdyn, os.path.join(path, "hgrc.remote_cache"))

        generationtime = ui.configint("configs", "generationtime")
        if (
            generationtime != -1
            and encoding.environ.get("HG_DEBUGDYNAMICCONFIG", "") != "1"
        ):
            mtimelimit = time.time() - generationtime
            if mtime < mtimelimit:
                # TODO: some how prevent kicking off the background process if
                # the file is read-only or if the previous kick offs failed.
                ui.debug("background generating dynamic config\n")
                env = encoding.environ.copy()
                # The environment variable prevents infiniteloops from
                # debugdynamicconfig kicking itself off, or doing it via
                # commands spawned from the telemetry wrapper.
                env["HG_DEBUGDYNAMICCONFIG"] = "1"
                runbgcommand(["hg", "--cwd", path, "debugdynamicconfig"], env)


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


def validatedynamicconfig(ui):
    if not (
        ui.configbool("configs", "loaddynamicconfig")
        and ui.configbool("configs", "validatedynamicconfig")
    ):
        return

    # As we migrate rc files into the dynamic configs, we want to verify that
    # the dynamic config produces the exact same settings as each rc file.
    # 'ensuresourcesupersets' will ensures that 1) the configs set by a given rc
    # file match exactly what the dynamic config produces, and 2) dynamic
    # configs do not produce any values that do not come from an rc file.
    #
    # The combination of these two checks ensures the dynamic configs match the
    # rc files eactly. If any config does not match, the dynamic config value is
    # removed, leaving us with the correct config value from the rc file.  The
    # mismatch is returned here for logging.
    #
    # Once all configs are migrated, we can delete the rc files and remove this
    # validation.
    try:
        from . import fb

        originalrcs = fb.ported_hgrcs
    except ImportError:
        originalrcs = []

    # Configs that are set in our legacy infrastructure and that we should
    # therefore validate. This list should shrink over time.
    legacylist = []
    for sectionkey in ui.configlist("configs", "legacylist", []):
        section, key = sectionkey.split(".", 1)
        legacylist.append((section, key))

    testrcs = ui.configlist("configs", "testdynamicconfigsubset")
    if testrcs:
        originalrcs.extend(testrcs)
    issues = ui._uiconfig._rcfg.ensure_location_supersets(
        "hgrc.dynamic", originalrcs, legacylist
    )

    for section, key, dynamic_value, file_value in issues:
        msg = _("Config mismatch: %s.%s has '%s' (dynamic) vs '%s' (file)\n") % (
            section,
            key,
            dynamic_value,
            file_value,
        )
        if ui.configbool("configs", "mismatchwarn") and not ui.plain():
            ui.warn(msg)

        samplerate = ui.configint("configs", "mismatchsampling")
        if random.randint(1, samplerate) == 1:
            reponame = ui.config("remotefilelog", "reponame")
            ui.log(
                "config_mismatch",
                msg,
                config="%s.%s" % (section, key),
                expected=file_value,
                actual=dynamic_value,
                repo=reponame or "unknown",
            )


def applydynamicconfig(ui, reponame, sharedpath):
    if ui.configbool("configs", "loaddynamicconfig"):
        dynamicconfig.applydynamicconfig(ui._uiconfig._rcfg._rcfg, reponame, sharedpath)

        validatedynamicconfig(ui)


def generatedynamicconfig(ui, reponame, sharedpath):
    if ui.configbool("configs", "loaddynamicconfig"):
        dynamicconfig.generatedynamicconfig(reponame, sharedpath)
