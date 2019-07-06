# metrics.py
#
# Copyright 2016 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# To log a new metric, add it to the list FASTMANIFEST_METRICS
# Then from the code, use metrics.metricscollector.get(repo) or
# metrics.metricscollector.getfromui(ui) to get a metrics `collector`.
# call collector.recordsample(metricsname, key=value, key2=value2, ...) to
# record a samples.
#
# When the command ends the sample will be relayed with ui.log unless
# it is in the list FASTMANIFEST_DONOTREPORT_METRICS.
# You would put a metrics in that list if you do some computation with hit
# and are not interested in the individual sample but only their aggregation.
# For example, if you want to record the cache hit ratio, you can record
# all the cache hit and cache miss, not report them but compute and report their
# ratio.
#
# To debug metrics use fastmanifest.debugmetrics = True, this will print
# the metrics collected for each command with ui.status at the end of each
# command.
from __future__ import absolute_import


FASTMANIFEST_DONOTREPORT_METRICS = set(
    ["cachehit", "diffcachehit", "filesnotincachehit"]
)

FASTMANIFEST_METRICS = set(
    [
        ## Individual Metrics
        # ondiskcachestats has information about the cache on disk
        # => keys are "bytes", "entries", "limit" and "freespace", all numbers,
        # freespace and limit are in MB
        "ondiskcachestats",
        # revsetsize is the number of revisions in the 'fastmanifesttocache()'
        # => key is "size", a number
        "revsetsize",
        # trigger is what caused caching to trigger
        # => keys is "source", one of ("commit", "remotenames", "bookmark")
        "trigger",
        # cacheoverflow, logs cache overflow event: not enough space in the
        # cache to store revisions, it will inform us on how to resize the
        # cache if needed
        # => key is "hit", always True
        "cacheoverflow",
        # The three followings are metrics that will be aggregated as ratio
        # they register cache hit and miss at different level: global, diff and
        # during filesnotin operations
        # => key is "hit", True or False, True is a cache hit, False a cache miss
        "cachehit",
        "diffcachehit",
        "filesnotincachehit",
        ## Aggregate Metrics
        # Cache hit ratio (global, diff and filesnotin), expressed as a percentage
        # so between 0 and 100. -1 means no operations.
        # => keys is "ratio", a number
        # examples:
        # -1 for cachehitratio => we never accessed a manifest for the command
        # 30 for cachehitratio => 30% of manifest access hit the cache
        # 45 for diffcachehitratio => 45% of manifest diffs hit the cache
        "cachehitratio",
        "diffcachehitratio",
        "filesnotincachehitratio",
    ]
)


class metricscollector(object):
    _instance = None

    @classmethod
    def get(cls):
        if not cls._instance:
            cls._instance = metricscollector()
        return cls._instance

    def __init__(self):
        self.samples = []

    def recordsample(self, kind, **kwargs):
        assert kind in FASTMANIFEST_METRICS
        self.samples.append((kind, kwargs))

    def mergesamples(self, collector):
        if collector is not self:
            self.samples.extend(collector.samples)
        return self

    def _addaggregatesamples(self):
        def _addhitratio(key, aggkey, dedupe=False):
            # Aggregate the cache hit and miss to build a hit ratio
            # store the ratio as aggkey : {ratio: ratio} in self.samples
            # If dedupe is set, will dedupe using the node field of each sample
            hitlist = (s for s in self.samples if s[0] == key and s[1]["hit"])
            misslist = (s for s in self.samples if s[0] == key and not s[1]["hit"])
            if dedupe:
                hit = len(set(s[1]["node"] for s in hitlist))
                miss = len(set(s[1]["node"] for s in misslist))
            else:
                hit = len(list(hitlist))
                miss = len(list(misslist))

            if miss + hit == 0:
                ratio = -1
            else:
                ratio = float(hit) * 100 / (miss + hit)

            data = {aggkey: int(ratio)}

            self.recordsample(aggkey, **data)

        _addhitratio("cachehit", "cachehitratio", dedupe=True)
        _addhitratio("diffcachehit", "diffcachehitratio")
        _addhitratio("filesnotincachehit", "filesnotincachehitratio")

    def logsamples(self, ui):
        self._addaggregatesamples()
        debug = ui.configbool("fastmanifest", "debugmetrics")
        if debug:
            ui.status(("[FM-METRICS] Begin metrics\n"))

        for kind, kwargs in self.samples:
            if kind in FASTMANIFEST_DONOTREPORT_METRICS:
                continue

            if debug:
                dispkw = kwargs
                # Not removing freespace and limit would make the output of
                # test machine dependant
                if "freespace" in kwargs:
                    del dispkw["freespace"]
                if "limit" in kwargs:
                    del dispkw["limit"]
                # Here we sort to make test output stable
                ui.status(
                    (
                        "[FM-METRICS] kind: %s, kwargs: %s\n"
                        % (kind, sorted(dispkw.items()))
                    )
                )
        if debug:
            ui.status(("[FM-METRICS] End metrics\n"))
