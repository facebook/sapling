# worker.py - master-slave parallelism support
#
# Copyright 2013 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from i18n import _
import os, util

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
