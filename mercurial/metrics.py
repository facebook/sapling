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

    def gauge(self, entity, key, value):
        pass

def client(ui):
    """Returns the appropriate metrics module"""
    # @fb-only: from mercurial.fb import fbmetrics 
    # @fb-only: return fbmetrics(ui)
    return metrics(ui) # @oss-only
