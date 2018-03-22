# Commit cloud
#
# Copyright 2018 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
""" sync changesets via the cloud

    [commitcloud]
    # type of commit cloud service to connect to
    # local or interngraph
    servicetype = local

    # location of the commit cloud service to connect to
    servicelocation = /path/to/dir

    # hostname to use for the system
    hostname = myhost

    # host of commitcloud proxy
    host = interngraph.intern.facebook.com

    # use oauth authentication
    oauth = true

    # user token
    # private user token for Commit Cloud generated on
    # https://our.intern.facebook.com/intern/oauth/
    # to be used with oauth = true
    user_token = *****************************

    # application id that identifies commit cloud in interngraph
    # to be used with  oauth = false
    app_id = 361121054385388

    # app token (temporarily, will be moved to another place)
    # secret token for interngraph (valid for commit cloud service only)
    # to be used with  oauth = false
    app_token = **********

    # SSL certificates
    certs = /etc/pki/tls/certs/fb_certs.pem
"""

from __future__ import absolute_import

from mercurial import (
    obsolete,
    util,
)

from . import commitcloudcommands

cmdtable = commitcloudcommands.cmdtable

colortable = {
    'commitcloud.hashtag': 'yellow',
}

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
