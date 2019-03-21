# metrics.py - generic metrics framework
#
#  Copyright Mercurial Contributors
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import


class metrics(object):
    """Abstract base class for metrics"""

    def __init__(self, ui):
        self.ui = ui
        self.stats = {}

    def gauge(self, key, value=1, entity=None):
        """If entity is None, log locally. Otherwise, send it to a global counter."""
        if entity is None:
            self.stats.setdefault(key, 0)
            self.stats[key] += value


def client(ui):
    """Returns the appropriate metrics module"""
    # @fb-only: from . import fb 

    # @fb-only: return fb.fbmetrics(ui) 
    return metrics(ui) # @oss-only
