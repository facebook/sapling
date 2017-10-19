# profiling.py - profiling functions
#
# Copyright 2016 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import contextlib

from .i18n import _
from . import (
    encoding,
    error,
    extensions,
    util,
)

def _loadprofiler(ui, profiler):
    """load profiler extension. return profile method, or None on failure"""
    extname = profiler
    extensions.loadall(ui, whitelist=[extname])
    try:
        mod = extensions.find(extname)
    except KeyError:
        return None
    else:
        return getattr(mod, 'profile', None)

@contextlib.contextmanager
def lsprofile(ui, fp):
    format = ui.config('profiling', 'format')
    field = ui.config('profiling', 'sort')
    limit = ui.configint('profiling', 'limit')
    climit = ui.configint('profiling', 'nested')

    if format not in ['text', 'kcachegrind']:
        ui.warn(_("unrecognized profiling format '%s'"
                    " - Ignored\n") % format)
        format = 'text'

    try:
        from . import lsprof
    except ImportError:
        raise error.Abort(_(
            'lsprof not available - install from '
            'http://codespeak.net/svn/user/arigo/hack/misc/lsprof/'))
    p = lsprof.Profiler()
    p.enable(subcalls=True)
    try:
        yield
    finally:
        p.disable()

        if format == 'kcachegrind':
            from . import lsprofcalltree
            calltree = lsprofcalltree.KCacheGrind(p)
            calltree.output(fp)
        else:
            # format == 'text'
            stats = lsprof.Stats(p.getstats())
            stats.sort(field)
            stats.pprint(limit=limit, file=fp, climit=climit)

@contextlib.contextmanager
def flameprofile(ui, fp):
    try:
        from flamegraph import flamegraph
    except ImportError:
        raise error.Abort(_(
            'flamegraph not available - install from '
            'https://github.com/evanhempel/python-flamegraph'))
    # developer config: profiling.freq
    freq = ui.configint('profiling', 'freq')
    filter_ = None
    collapse_recursion = True
    thread = flamegraph.ProfileThread(fp, 1.0 / freq,
                                      filter_, collapse_recursion)
    start_time = util.timer()
    try:
        thread.start()
        yield
    finally:
        thread.stop()
        thread.join()
        print('Collected %d stack frames (%d unique) in %2.2f seconds.' % (
            util.timer() - start_time, thread.num_frames(),
            thread.num_frames(unique=True)))

@contextlib.contextmanager
def statprofile(ui, fp):
    from . import statprof

    freq = ui.configint('profiling', 'freq')
    if freq > 0:
        # Cannot reset when profiler is already active. So silently no-op.
        if statprof.state.profile_level == 0:
            statprof.reset(freq)
    else:
        ui.warn(_("invalid sampling frequency '%s' - ignoring\n") % freq)

    statprof.start(mechanism='thread')

    try:
        yield
    finally:
        data = statprof.stop()

        profformat = ui.config('profiling', 'statformat')

        formats = {
            'byline': statprof.DisplayFormats.ByLine,
            'bymethod': statprof.DisplayFormats.ByMethod,
            'hotpath': statprof.DisplayFormats.Hotpath,
            'json': statprof.DisplayFormats.Json,
            'chrome': statprof.DisplayFormats.Chrome,
        }

        if profformat in formats:
            displayformat = formats[profformat]
        else:
            ui.warn(_('unknown profiler output format: %s\n') % profformat)
            displayformat = statprof.DisplayFormats.Hotpath

        kwargs = {}

        def fraction(s):
            if isinstance(s, (float, int)):
                return float(s)
            if s.endswith('%'):
                v = float(s[:-1]) / 100
            else:
                v = float(s)
            if 0 <= v <= 1:
                return v
            raise ValueError(s)

        if profformat == 'chrome':
            showmin = ui.configwith(fraction, 'profiling', 'showmin', 0.005)
            showmax = ui.configwith(fraction, 'profiling', 'showmax')
            kwargs.update(minthreshold=showmin, maxthreshold=showmax)
        elif profformat == 'hotpath':
            # inconsistent config: profiling.showmin
            limit = ui.configwith(fraction, 'profiling', 'showmin', 0.05)
            kwargs['limit'] = limit

        statprof.display(fp, data=data, format=displayformat, **kwargs)

class profile(object):
    """Start profiling.

    Profiling is active when the context manager is active. When the context
    manager exits, profiling results will be written to the configured output.
    """
    def __init__(self, ui, enabled=True):
        self._ui = ui
        self._output = None
        self._fp = None
        self._fpdoclose = True
        self._profiler = None
        self._enabled = enabled
        self._entered = False
        self._started = False

    def __enter__(self):
        self._entered = True
        if self._enabled:
            self.start()
        return self

    def start(self):
        """Start profiling.

        The profiling will stop at the context exit.

        If the profiler was already started, this has no effect."""
        if not self._entered:
            raise error.ProgrammingError()
        if self._started:
            return
        self._started = True
        profiler = encoding.environ.get('HGPROF')
        proffn = None
        if profiler is None:
            profiler = self._ui.config('profiling', 'type')
        if profiler not in ('ls', 'stat', 'flame'):
            # try load profiler from extension with the same name
            proffn = _loadprofiler(self._ui, profiler)
            if proffn is None:
                self._ui.warn(_("unrecognized profiler '%s' - ignored\n")
                              % profiler)
                profiler = 'stat'

        self._output = self._ui.config('profiling', 'output')

        try:
            if self._output == 'blackbox':
                self._fp = util.stringio()
            elif self._output:
                path = self._ui.expandpath(self._output)
                self._fp = open(path, 'wb')
            else:
                self._fpdoclose = False
                self._fp = self._ui.ferr

            if proffn is not None:
                pass
            elif profiler == 'ls':
                proffn = lsprofile
            elif profiler == 'flame':
                proffn = flameprofile
            else:
                proffn = statprofile

            self._profiler = proffn(self._ui, self._fp)
            self._profiler.__enter__()
        except: # re-raises
            self._closefp()
            raise

    def __exit__(self, exception_type, exception_value, traceback):
        propagate = None
        if self._profiler is not None:
            propagate = self._profiler.__exit__(exception_type, exception_value,
                                                traceback)
            if self._output == 'blackbox':
                val = 'Profile:\n%s' % self._fp.getvalue()
                # ui.log treats the input as a format string,
                # so we need to escape any % signs.
                val = val.replace('%', '%%')
                self._ui.log('profile', val)
        self._closefp()
        return propagate

    def _closefp(self):
        if self._fpdoclose and self._fp is not None:
            self._fp.close()
