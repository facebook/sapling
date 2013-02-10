# worker.py - master-slave parallelism support
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import os, signal, sys, util

def countcpus():
    '''try to count the number of CPUs on the system'''

    # posix
    try:
        n = int(os.sysconf('SC_NPROCESSORS_ONLN'))
        if n > 0:
            return n
    except (AttributeError, ValueError):
        pass

    # windows
    try:
        n = int(os.environ['NUMBER_OF_PROCESSORS'])
        if n > 0:
            return n
    except (KeyError, ValueError):
        pass

    return 1

def _numworkers(ui):
    s = ui.config('worker', 'numcpus')
    if s:
        try:
            n = int(s)
            if n >= 1:
                return n
        except ValueError:
            raise util.Abort(_('number of cpus must be an integer'))
    return min(max(countcpus(), 4), 32)

if os.name == 'posix':
    _startupcost = 0.01
else:
    _startupcost = 1e30

def worthwhile(ui, costperop, nops):
    '''try to determine whether the benefit of multiple processes can
    outweigh the cost of starting them'''
    linear = costperop * nops
    workers = _numworkers(ui)
    benefit = linear - (_startupcost * workers + linear / workers)
    return benefit >= 0.15

def worker(ui, costperarg, func, staticargs, args):
    '''run a function, possibly in parallel in multiple worker
    processes.

    returns a progress iterator

    costperarg - cost of a single task

    func - function to run

    staticargs - arguments to pass to every invocation of the function

    args - arguments to split into chunks, to pass to individual
    workers
    '''
    if worthwhile(ui, costperarg, len(args)):
        return _platformworker(ui, func, staticargs, args)
    return func(*staticargs + (args,))

def _posixworker(ui, func, staticargs, args):
    rfd, wfd = os.pipe()
    workers = _numworkers(ui)
    for pargs in partition(args, workers):
        pid = os.fork()
        if pid == 0:
            try:
                os.close(rfd)
                for i, item in func(*(staticargs + (pargs,))):
                    os.write(wfd, '%d %s\n' % (i, item))
                os._exit(0)
            except KeyboardInterrupt:
                os._exit(255)
    os.close(wfd)
    fp = os.fdopen(rfd, 'rb', 0)
    oldhandler = signal.getsignal(signal.SIGINT)
    signal.signal(signal.SIGINT, signal.SIG_IGN)
    def cleanup():
        # python 2.4 is too dumb for try/yield/finally
        signal.signal(signal.SIGINT, oldhandler)
        problems = 0
        for i in xrange(workers):
            problems |= os.wait()[1]
        if problems:
            sys.exit(1)
    try:
        for line in fp:
            l = line.split(' ', 1)
            yield int(l[0]), l[1][:-1]
    except: # re-raises
        cleanup()
        raise
    cleanup()

if os.name != 'nt':
    _platformworker = _posixworker

def partition(lst, nslices):
    '''partition a list into N slices of equal size'''
    n = len(lst)
    chunk, slop = n / nslices, n % nslices
    end = 0
    for i in xrange(nslices):
        start = end
        end = start + chunk
        if slop:
            end += 1
            slop -= 1
        yield lst[start:end]
