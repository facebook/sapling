# Portions Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# profiling.py - profiling functions
#
# Copyright 2016 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import contextlib
import errno
import time

from . import blackbox, error, extensions, pycompat, util
from .i18n import _


def _loadprofiler(ui, profiler):
    """load profiler extension. return profile method, or None on failure"""
    extname = profiler
    extensions.loadall(ui, whitelist=[extname])
    try:
        mod = extensions.find(extname)
    except KeyError:
        return None
    else:
        return getattr(mod, "profile", None)


@contextlib.contextmanager
def lsprofile(ui, fp, section):
    format = ui.config(section, "format")
    field = ui.config(section, "sort")
    limit = ui.configint(section, "limit")
    climit = ui.configint(section, "nested")

    if format not in ["text", "kcachegrind"]:
        ui.warn(_("unrecognized profiling format '%s'" " - Ignored\n") % format)
        format = "text"

    try:
        from . import lsprof
    except ImportError:
        raise error.Abort(
            _(
                "lsprof not available - install from "
                "http://codespeak.net/svn/user/arigo/hack/misc/lsprof/"
            )
        )
    p = lsprof.Profiler()
    p.enable(subcalls=True)
    try:
        yield
    finally:
        p.disable()

        if format == "kcachegrind":
            from . import lsprofcalltree

            calltree = lsprofcalltree.KCacheGrind(p)
            calltree.output(fp)
        else:
            # format == 'text'
            stats = lsprof.Stats(p.getstats())
            stats.sort(field)
            stats.pprint(limit=limit, file=fp, climit=climit)


@contextlib.contextmanager
def flameprofile(ui, fp, section):
    try:
        # pyre-fixme[21]: Could not find `flamegraph`.
        from flamegraph import flamegraph
    except ImportError:
        raise error.Abort(
            _(
                "flamegraph not available - install from "
                "https://github.com/evanhempel/python-flamegraph"
            )
        )
    # developer config: profiling.freq
    freq = ui.configint(section, "freq")
    filter_ = None
    collapse_recursion = True
    thread = flamegraph.ProfileThread(fp, 1.0 / freq, filter_, collapse_recursion)
    start_time = util.timer()
    try:
        thread.start()
        yield
    finally:
        thread.stop()
        thread.join()
        print(
            "Collected %d stack frames (%d unique) in %2.2f seconds."
            % (
                util.timer() - start_time,
                thread.num_frames(),
                thread.num_frames(unique=True),
            )
        )


@contextlib.contextmanager
def statprofile(ui, fp, section):
    from . import statprof

    freq = ui.configwith(float, section, "freq")
    if freq > 0:
        # Cannot reset when profiler is already active. So silently no-op.
        if statprof.state.profile_level == 0:
            statprof.reset(freq)
    else:
        ui.warn(_("invalid sampling frequency '%s' - ignoring\n") % freq)

    statprof.start(mechanism="thread")

    try:
        yield
    finally:
        data = statprof.stop()

        profformat = ui.config(section, "statformat")

        formats = {
            "byline": statprof.DisplayFormats.ByLine,
            "bymethod": statprof.DisplayFormats.ByMethod,
            "hotpath": statprof.DisplayFormats.Hotpath,
            "json": statprof.DisplayFormats.Json,
            "chrome": statprof.DisplayFormats.Chrome,
        }

        if profformat in formats:
            displayformat = formats[profformat]
        else:
            ui.warn(_("unknown profiler output format: %s\n") % profformat)
            displayformat = statprof.DisplayFormats.Hotpath

        kwargs = {}

        def fraction(s):
            if isinstance(s, (float, int)):
                return float(s)
            if s.endswith("%"):
                v = float(s[:-1]) / 100
            else:
                v = float(s)
            if 0 <= v <= 1:
                return v
            raise ValueError(s)

        if profformat == "chrome":
            showmin = ui.configwith(fraction, section, "showmin", 0.005)
            showmax = ui.configwith(fraction, section, "showmax")
            kwargs.update(minthreshold=showmin, maxthreshold=showmax)
        elif profformat == "hotpath":
            # inconsistent config: profiling.showmin
            limit = ui.configwith(fraction, section, "showmin", 0.05)
            kwargs["limit"] = limit

        if ui.config(section, "output"):
            kwargs["color"] = False

        statprof.display(fp, data=data, format=displayformat, **kwargs)


class profile(object):
    """Start profiling.

    Profiling is active when the context manager is active. When the context
    manager exits, profiling results will be written to the configured output.
    """

    def __init__(self, ui):
        self._ui = ui
        self._fp = None
        self._profiler = None
        self._entered = False
        self._started = False
        self._section = "profiling"

    def __enter__(self):
        self._entered = True
        sections = sorted(
            s for s in self._ui.configsections() if s.split(":", 1)[0] == "profiling"
        )
        for section in sections:
            if self._ui.configbool(section, "enabled"):
                self._section = section
                self.start()
                break
        return self

    def start(self):
        """Start profiling.

        The profiling will stop at the context exit.

        If the profiler was already started, this has no effect."""
        if not self._entered:
            raise error.ProgrammingError()
        if self._started:
            return
        self._started = time.time()
        proffn = None
        profiler = self._ui.config(self._section, "type")
        if profiler not in ("ls", "stat", "flame"):
            # try load profiler from extension with the same name
            proffn = _loadprofiler(self._ui, profiler)
            if proffn is None:
                self._ui.warn(_("unrecognized profiler '%s' - ignored\n") % profiler)
                profiler = "stat"

        self._fp = util.stringio()

        if proffn is not None:
            pass
        elif profiler == "ls":
            proffn = lsprofile
        elif profiler == "flame":
            proffn = flameprofile
        else:
            proffn = statprofile

        self._profiler = proffn(self._ui, self._fp, self._section)
        self._profiler.__enter__()

    def __exit__(self, exception_type, exception_value, traceback):
        propagate = None
        if self._profiler is not None:
            propagate = self._profiler.__exit__(
                exception_type, exception_value, traceback
            )
            elapsed = time.time() - self._started
            if elapsed >= self._ui.configint(self._section, "minelapsed"):
                output = self._ui.config(self._section, "output")
                content = self._fp.getvalue()
                if output == "blackbox":
                    blackbox.log({"profile": {"msg": content}})
                elif output:
                    path = self._ui.expandpath(output)
                    try:
                        with open(path, "wb") as f:
                            f.write(content)
                    except IOError as e:
                        # CreateFile(.., CREATE_ALWAYS, ...) can fail
                        # for existing "hidden" or "system" files.
                        # See D8099420.
                        if pycompat.iswindows and e.errno == errno.EACCES:
                            with open(path, "r+b") as f:
                                f.write(content)
                else:
                    self._ui.write_err(content)
        return propagate
