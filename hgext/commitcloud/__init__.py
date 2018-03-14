# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" sync changesets via the cloud

    [commitcloud]
    # type of commit cloud service to connect to
    servicetype = local

    # location of the commit cloud service to connect to
    servicelocation = /path/to/dir

    # hostname to use for the system
    hostname = myhost
"""

from __future__ import absolute_import

from mercurial import (
    obsolete,
    util,
)

from . import commitcloudcommands

cmdtable = commitcloudcommands.cmdtable

def reposetup(ui, repo):
    def finalize(tr):
        if util.safehasattr(tr, '_commitcloudskippendingobsmarkers'):
            return
        markers = tr.changes['obsmarkers']
        if markers:
            f = tr.opener('commitcloudpendingobsmarkers', 'ab')
            try:
                offset = f.tell()
                tr.add('commitcloudpendingobsmarkers', offset)
                # offset == 0: new file - add the version header
                data = b''.join(obsolete.encodemarkers(markers, offset == 0,
                                                       obsolete._fm1version))
                f.write(data)
            finally:
                f.close()

    class commitcloudrepo(repo.__class__):
        def transaction(self, *args, **kwargs):
            tr = super(commitcloudrepo, self).transaction(*args, **kwargs)
            tr.addfinalize('commitcloudobsmarkers', finalize)
            return tr
    repo.__class__ = commitcloudrepo
