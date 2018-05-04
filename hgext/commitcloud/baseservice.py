# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

# Standard Library
import abc
import collections
import json

# Mercurial
from mercurial import node

abstractmethod = abc.abstractmethod
References = collections.namedtuple('References',
                                    'version heads bookmarks obsmarkers')

class BaseService(object):
    __metaclass__ = abc.ABCMeta

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
        newobsmarkers = [
            (
                node.bin(m['pred']),
                tuple(node.bin(s) for s in m['succs']),
                m['flags'],
                tuple((k.encode('utf-8'), v.encode('utf-8'))
                      for k, v in json.loads(m['meta'])),
                (float(m['date']), m['tz']),
                tuple(node.bin(p) for p in m['predparents']),
            )
            for m in data['new_obsmarkers_data']
        ]

        return References(version, newheads, newbookmarks, newobsmarkers)

    def _encodedmarkers(self, obsmarkers):
        # pred, succs, flags, metadata, date, parents = marker
        return [
            {
                "pred": node.hex(m[0]),
                "succs": [node.hex(s) for s in m[1]],
                "predparents": [node.hex(p) for p in m[5]] if m[5] else [],
                "flags": m[2],
                "date": repr(m[4][0]),
                "tz": m[4][1],
                "meta": json.dumps(m[3]),
            }
            for m in obsmarkers]

    @abstractmethod
    def requiresauthentication(self):
        """Returns True if the service requires authentication tokens"""

    @abstractmethod
    def check(self):
        """Returns True if the connection to the service is ok"""

    @abstractmethod
    def updatereferences(self, reponame, workspace, version, oldheads, newheads,
                         oldbookmarks, newbookmarks, newobsmarkers):
        """Updates the references to a new version.

        If the update was successful, returns `(True, references)`, where
        `references` is a References object containing the new version.

        If the update was not successful, returns `(False, references)`,
        where `references` is a References object containing the current
        version, including its heads and bookmarks.
        """

    @abstractmethod
    def getreferences(self, reponame, workspace, baseversion):
        """Gets the current references if they differ from the base version
        """
