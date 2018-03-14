# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import json
import os

from mercurial import (
    error,
)

References = collections.namedtuple('References', 'version heads bookmarks')

class LocalService(object):
    """Local commit-cloud service implemented using files on disk.

    There is no locking, so this is suitable only for use in unit tests.
    """
    def __init__(self, ui):
        self._ui = ui
        self.path = ui.config('commitcloud', 'servicelocation')
        if not self.path or not os.path.isdir(self.path):
            msg = 'Invalid commitcloud.servicelocation: %s' % self.path
            raise error.Abort(msg)

    def _load(self):
        filename = os.path.join(self.path, 'commitcloudservicedb')
        if os.path.exists(filename):
            with open(filename) as f:
                data = json.load(f)
                return data
        else:
            return {'version': 0, 'heads': [], 'bookmarks': {},
                    'obsmarkers': {}}

    def _save(self, data):
        filename = os.path.join(self.path, 'commitcloudservicedb')
        with open(filename, 'w') as f:
            json.dump(data, f)

    def _makereferences(self, data):
        """Makes a References object from JSON data"""
        version = data['version']
        newheads = [h.encode() for h in data['heads']]
        newbookmarks = {n.encode('utf-8'): v.encode()
                        for n, v in data['bookmarks'].items()}
        return References(version, newheads, newbookmarks)

    def getreferences(self, baseversion):
        """Gets the current references if they differ from the base version
        """
        data = self._load()
        version = data['version']
        if version == baseversion:
            self._ui.debug(
                'commitcloud local service: '
                'get_references for current version %s\n' % version)
            return References(version, None, None)
        else:
            self._ui.debug(
                'commitcloud local service: '
                'get_references for versions from %s to %s\n'
                % (baseversion, version))
            return self._makereferences(data)

    def updatereferences(self, version, oldheads, newheads, oldbookmarks,
                          newbookmarks):
        """Updates the references to a new version.

        If the update was successful, returns `(True, references)`, where
        `references` is a References object containing the new version.

        If the update was not successful, returns `(False, references)`,
        where `references` is a References object containing the current
        version, including its heads and bookmarks.
        """
        data = self._load()
        if version != data['version']:
            return False, self._makereferences(data)
        data['version'] = data['version'] + 1
        data['heads'] = newheads
        data['bookmarks'] = newbookmarks
        self._ui.debug(
            'commitcloud local service: '
            'update_references to %s (%s heads, %s bookmarks)\n'
            % (data['version'], len(data['heads']), len(data['bookmarks'])))
        self._save(data)
        return True, References(data['version'], None, None)

def get(ui):
    servicetype = ui.config('commitcloud', 'servicetype')
    if servicetype == 'local':
        return LocalService(ui)
    else:
        msg = 'Unrecognized commitcloud.servicetype: %s' % servicetype
        raise error.Abort(msg)
