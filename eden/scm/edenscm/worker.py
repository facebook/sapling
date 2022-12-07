# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# worker.py - leader-follower parallelism support
#
# Copyright Olivia Mackall <olivia@selenic.com> and others
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import os
import threading
import time

from . import encoding, error, pycompat, util
from .i18n import _


def countcpus():
    """try to count the number of CPUs on the system"""

    # posix
    try:
        n = int(os.sysconf(r"SC_NPROCESSORS_ONLN"))
        if n > 0:
            return n
    except (AttributeError, ValueError):
        pass

    # windows
    try:
        n = int(encoding.environ["NUMBER_OF_PROCESSORS"])
        if n > 0:
            return n
    except (KeyError, ValueError):
        pass

    return 1


def _numworkers(ui):
    s = ui.config("worker", "numcpus")
    if s:
        try:
            n = int(s)
            if n >= 1:
                return n
        except ValueError:
            raise error.Abort(_("number of cpus must be an integer"))
    return min(max(countcpus(), 4), 32)


if pycompat.isposix or pycompat.iswindows:
    _startupcost = 0.01
else:
    _startupcost = 1e30


def worthwhile(ui, costperop, nops):
    """try to determine whether the benefit of multiple processes can
    outweigh the cost of starting them"""
    linear = costperop * nops
    workers = _numworkers(ui)
    benefit = linear - (_startupcost * workers + linear / workers)
    return benefit >= 0.15


def worker(ui, costperarg, func, staticargs, args, preferthreads=False, callsite=None):
    """run a function, possibly in parallel in multiple worker threads.

    returns a progress iterator

    costperarg - cost of a single task

    func - function to run

    staticargs - arguments to pass to every invocation of the function

    args - arguments to split into chunks, to pass to individual
    workers

    preferthreads - use threads instead of processes

    callsite - where this worker function is being called
    """
    workerenabled = ui.configbool("worker", "enabled")
    callsiteenabled = callsite in ui.configlist("worker", "_enabledcallsites")
    enabled = workerenabled or callsiteenabled
    if enabled and worthwhile(ui, costperarg, len(args)):
        return _threadedworker(ui, func, staticargs, args)
    return func(*staticargs + (args,))


def _threadedworker(ui, func, staticargs, args):
    class Worker(threading.Thread):
        def __init__(
            self,
            taskqueue,
            resultqueue,
            func,
            staticargs,
            group=None,
            target=None,
            name=None,
        ):
            threading.Thread.__init__(self, group=group, target=target, name=name)
            self._taskqueue = taskqueue
            self._resultqueue = resultqueue
            self._func = func
            self._staticargs = staticargs
            self._interrupted = False
            self.daemon = True
            self.exception = None

        def interrupt(self):
            self._interrupted = True

        def run(self) -> None:
            try:
                while not self._taskqueue.empty():
                    try:
                        args = self._taskqueue.get_nowait()
                        for res in self._func(*self._staticargs + (args,)):
                            self._resultqueue.put(res)
                            # threading doesn't provide a native way to
                            # interrupt execution. handle it manually at every
                            # iteration.
                            if self._interrupted:
                                return
                    except util.empty:
                        break
            except Exception as e:
                # store the exception such that the main thread can resurface
                # it as if the func was running without workers.
                self.exception = e
                raise

    threads = []

    def trykillworkers():
        # Allow up to 1 second to clean worker threads nicely
        cleanupend = time.time() + 1
        for t in threads:
            t.interrupt()
        for t in threads:
            remainingtime = cleanupend - time.time()
            t.join(remainingtime)
            if t.is_alive():
                # pass over the workers joining failure. it is more
                # important to surface the inital exception than the
                # fact that one of workers may be processing a large
                # task and does not get to handle the interruption.
                ui.warn(
                    _("failed to kill worker threads while " "handling an exception\n")
                )
                return

    workers = _numworkers(ui)
    resultqueue = util.queue()
    taskqueue = util.queue()
    # partition work to more pieces than workers to minimize the chance
    # of uneven distribution of large tasks between the workers
    for pargs in partition(args, workers * 20):
        taskqueue.put(pargs)
    for _i in range(workers):
        t = Worker(taskqueue, resultqueue, func, staticargs)
        threads.append(t)
        t.start()
    try:
        while len(threads) > 0:
            while not resultqueue.empty():
                yield resultqueue.get()
            threads[0].join(0.05)
            finishedthreads = [_t for _t in threads if not _t.is_alive()]
            for t in finishedthreads:
                if t.exception is not None:
                    raise t.exception
                threads.remove(t)
    except (Exception, KeyboardInterrupt):  # re-raises
        trykillworkers()
        raise
    while not resultqueue.empty():
        yield resultqueue.get()


def partition(lst, nslices):
    """partition a list into N slices of roughly equal size

    The current strategy takes every Nth element from the input. If
    we ever write workers that need to preserve grouping in input
    we should consider allowing callers to specify a partition strategy.

    mpm is not a fan of this partitioning strategy when files are involved.
    In his words:

        Single-threaded Mercurial makes a point of creating and visiting
        files in a fixed order (alphabetical). When creating files in order,
        a typical filesystem is likely to allocate them on nearby regions on
        disk. Thus, when revisiting in the same order, locality is maximized
        and various forms of OS and disk-level caching and read-ahead get a
        chance to work.

        This effect can be quite significant on spinning disks. I discovered it
        circa Mercurial v0.4 when revlogs were named by hashes of filenames.
        Tarring a repo and copying it to another disk effectively randomized
        the revlog ordering on disk by sorting the revlogs by hash and suddenly
        performance of my kernel checkout benchmark dropped by ~10x because the
        "working set" of sectors visited no longer fit in the drive's cache and
        the workload switched from streaming to random I/O.

        What we should really be doing is have workers read filenames from a
        ordered queue. This preserves locality and also keeps any worker from
        getting more than one file out of balance.
    """
    for i in range(nslices):
        yield lst[i::nslices]
