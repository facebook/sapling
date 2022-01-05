# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# metrics.py - generic metrics framework

from __future__ import absolute_import

from bindings import hgmetrics


class metrics(object):
    """Abstract base class for metrics"""

    def __init__(self, ui):
        self.ui = ui

    def gauge(self, key, value=1, entity=None):
        """If entity is None, log locally. Otherwise, send it to a global counter."""
        if entity is None:
            hgmetrics.incrementcounter(key, value)


def client(ui):
    """Returns the appropriate metrics module"""
    # @fb-only: from . import fb 

    # @fb-only: return fb.fbmetrics(ui) 
    return metrics(ui) # @oss-only
