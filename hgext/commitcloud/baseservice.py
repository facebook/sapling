# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import
from abc import ABCMeta, abstractmethod

import base64
import collections

from mercurial import (
    obsolete,
)

References = collections.namedtuple('References',
                                    'version heads bookmarks obsmarkers')

class BaseService(object):
    __metaclass__ = ABCMeta
    def _makereferences(self, data):
        """Makes a References object from JSON data

            JSON data must represent json serialization of
            //scm/commitcloud/if/CommitCloudService.thrift
            struct ReferencesData

            Result represents struct References from this module
        """
        version = data['version']
        newheads = [h.encode('ascii') for h in data['heads']]
        newbookmarks = {
            n.encode('utf-8'): v.encode('ascii')
            for n, v in data['bookmarks'].items()
        }
        decobsmarkers = b''.join([
            base64.b64decode(m)
            for m in data['new_obsmarkers']
        ])
        newobsmarkers = obsolete._fm1readmarkers(
            decobsmarkers, 0, len(decobsmarkers))
        return References(version, newheads, newbookmarks, newobsmarkers)

    def _encodedmarkers(self, obsmarkers):
        return [base64.b64encode(m) for m in obsolete.encodemarkers(
                    obsmarkers, False, obsolete._fm1version)]

    @abstractmethod
    def updatereferences(self, version, oldheads, newheads, oldbookmarks,
                         newbookmarks, newobsmarkers):
        """Updates the references to a new version.

        If the update was successful, returns `(True, references)`, where
        `references` is a References object containing the new version.

        If the update was not successful, returns `(False, references)`,
        where `references` is a References object containing the current
        version, including its heads and bookmarks.
        """

    @abstractmethod
    def getreferences(self, baseversion):
        """Gets the current references if they differ from the base version
        """
