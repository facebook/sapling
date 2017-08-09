from __future__ import absolute_import
import json
import time

from mercurial import progress, util

"""allows users to have JSON progress bar information written to a path

Controlled by the `ui.progressfile` config. Mercurial will overwrite this file
each time the progress bar is updated. It is not affected by HGPLAIN since it
does not write to stdout.

The schema of this file is (JSON):

- topics: array of topics from oldest to newest. (last is always the active one)
- state: map of topic names to objects with keys:
    - topic (e.g. "changesets", "manifests")
    - pos: which item number out of <total> we're processing
    - total: total number of items (can change!)
    - unit: name of the type of unit being processed (e.g., "changeset")
    - item: the active item being processed (e.g., "changeset #5")
    - active: whether this is the currently active progress bar
    - units_per_sec: if active, how many <unit>s per sec we're processing
    - speed_str: if active, a human-readable string of how many <unit>s per sec
        we're processing
    - estimate_sec: an estimate of how much time is left, in seconds
    - estimate_str: if active, a human-readable string estimate of how much time
        is left (e.g. "2m30s")

config example::

    [progress]
    # Where to write progress information
    statefile = /some/path/to/file
"""
testedwith = 'ships-with-fb-hgext'

class progbarwithfile(progress.progbar):
    def progress(self, topic, pos, item='', unit='', total=None):
        super(progbarwithfile, self).progress(topic, pos, item, unit, total)
        self.writeprogress(time.time())

    def writeprogress(self, now):
        progressfile = self.ui.config('progress', 'statefile')
        if not progressfile:
            return

        topics = {}
        for topic in self.topicstates.keys():
            pos, item, unit, total = self.topicstates[topic]
            isactive = topic == self.curtopic
            cullempty = lambda str: str if str else None
            info = {
                'topic': topic,
                'pos': pos,
                'total': total,
                'unit': cullempty(unit),
                'item': cullempty(item),

                'active': isactive,
                'units_per_sec': None,
                'speed_str': None,
                'estimate_sec': None,
                'estimate_str': None,
            }
            if isactive:
                info['units_per_sec'] = cullempty(self._speed(topic, pos, now))
                info['estimate_sec'] = cullempty(self._estimate(
                    topic, pos, total, now))
                info['speed_str'] = cullempty(self.speed(topic, pos, unit, now))
                info['estimate_str'] = cullempty(self.estimate(
                    topic, pos, total, now))
            topics[topic] = info

        text = json.dumps({
            'state': topics,
            'topics': self.topics,
        }, sort_keys=True)
        try:
            with open(progressfile, 'w+') as f:
                f.write(text)
        except (IOError, OSError):
            pass

    # These duplicate some logic in progress.py.
    def _estimate(self, topic, pos, total, now):
        if total is None:
            return None
        initialpos = self.startvals[topic]
        target = total - initialpos
        delta = pos - initialpos
        if delta > 0:
            elapsed = now - self.starttimes[topic]
            return int((elapsed * (target - delta)) // delta + 1)
        return None

    def _speed(self, topic, pos, now):
        initialpos = self.startvals[topic]
        delta = pos - initialpos
        elapsed = now - self.starttimes[topic]
        return int(delta / elapsed)

def uisetup(ui):
    progbar = progbarwithfile(ui)
    class progressfileui(ui.__class__):
        """Redirects _progbar to our version, which always outputs if the config
        is set, and calls the default progbar if plain mode is off.
        """
        @util.propertycache
        def _progbar(self):
            return progbar

    ui.__class__ = progressfileui
