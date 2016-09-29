# perf.py - asv benchmarks using contrib/perf.py extension
#
# Copyright 2016 Logilab SA <contact@logilab.fr>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from . import perfbench

@perfbench()
def track_tags(perf):
    return perf("perftags")

@perfbench()
def track_status(perf):
    return perf("perfstatus", unknown=False)

@perfbench(params=[('rev', ['1000', '10000', 'tip'])])
def track_manifest(perf, rev):
    return perf("perfmanifest", rev)

@perfbench()
def track_heads(perf):
    return perf("perfheads")
