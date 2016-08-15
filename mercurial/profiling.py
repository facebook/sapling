# profiling.py - profiling functions
#
# Copyright 2016 Gregory Szorc <gregory.szorc@gmail.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import, print_function

import contextlib
import os
import sys
import time

from .i18n import _
from . import (
    error,
    util,
)

@contextlib.contextmanager
def lsprofile(ui, fp):
    format = ui.config('profiling', 'format', default='text')
    field = ui.config('profiling', 'sort', default='inlinetime')
    limit = ui.configint('profiling', 'limit', default=30)
    climit = ui.configint('profiling', 'nested', default=0)

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
    freq = ui.configint('profiling', 'freq', default=1000)
    filter_ = None
    collapse_recursion = True
    thread = flamegraph.ProfileThread(fp, 1.0 / freq,
                                      filter_, collapse_recursion)
    start_time = time.clock()
    try:
        thread.start()
        yield
    finally:
        thread.stop()
        thread.join()
        print('Collected %d stack frames (%d unique) in %2.2f seconds.' % (
            time.clock() - start_time, thread.num_frames(),
            thread.num_frames(unique=True)))

@contextlib.contextmanager
def statprofile(ui, fp):
    try:
        import statprof
    except ImportError:
        raise error.Abort(_(
            'statprof not available - install using "easy_install statprof"'))

    freq = ui.configint('profiling', 'freq', default=1000)
    if freq > 0:
        # Cannot reset when profiler is already active. So silently no-op.
        if statprof.state.profile_level == 0:
            statprof.reset(freq)
    else:
        ui.warn(_("invalid sampling frequency '%s' - ignoring\n") % freq)

    statprof.start()
    try:
        yield
    finally:
        statprof.stop()
        statprof.display(fp)

@contextlib.contextmanager
def profile(ui):
    """Start profiling.

    Profiling is active when the context manager is active. When the context
    manager exits, profiling results will be written to the configured output.
    """
    profiler = os.getenv('HGPROF')
    if profiler is None:
        profiler = ui.config('profiling', 'type', default='ls')
    if profiler not in ('ls', 'stat', 'flame'):
        ui.warn(_("unrecognized profiler '%s' - ignored\n") % profiler)
        profiler = 'ls'

    output = ui.config('profiling', 'output')

    if output == 'blackbox':
        fp = util.stringio()
    elif output:
        path = ui.expandpath(output)
        fp = open(path, 'wb')
    else:
        fp = sys.stderr

    try:
        if profiler == 'ls':
            proffn = lsprofile
        elif profiler == 'flame':
            proffn = flameprofile
        else:
            proffn = statprofile

        with proffn(ui, fp):
            yield

    finally:
        if output:
            if output == 'blackbox':
                val = 'Profile:\n%s' % fp.getvalue()
                # ui.log treats the input as a format string,
                # so we need to escape any % signs.
                val = val.replace('%', '%%')
                ui.log('profile', val)
            fp.close()

@contextlib.contextmanager
def maybeprofile(ui):
    """Profile if enabled, else do nothing.

    This context manager can be used to optionally profile if profiling
    is enabled. Otherwise, it does nothing.

    The purpose of this context manager is to make calling code simpler:
    just use a single code path for calling into code you may want to profile
    and this function determines whether to start profiling.
    """
    if ui.configbool('profiling', 'enabled'):
        with profile(ui):
            yield
    else:
        yield
