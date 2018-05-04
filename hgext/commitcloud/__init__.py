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

    # user token
    # private user token to access the commit cloud service
    user_token = *****************************

    # application id that identifies commit cloud in interngraph
    app_id = 361121054385388

    # SSL certificates
    certs = /etc/pki/tls/certs/fb_certs.pem

    # help message to provide instruction on registration process
    auth_help = please obtain an authentication token from https://example.com/

    # custom path to store authentication token (may be used for testing)
    # the path should exist
    user_token_path = /tmp

    # owner team, used for help messages
    owner_team = "The Source Control Team"

    # update to a new revision if the current revision has been moved
    updateonmove = true

    # option to print requests to Commit Cloud web endpoint (in debug level)
    debugrequests = true
"""

from __future__ import absolute_import

from mercurial.i18n import _

from mercurial import (
    error,
    extensions,
    obsolete,
    util,
)

from . import (
    commitcloudcommands,
    commitcloudcommon,
    commitcloudutil,
)

cmdtable = commitcloudcommands.cmdtable

colortable = {
    'commitcloud.tag': 'yellow',
    'commitcloud.team': 'bold',
}

def _smartlogbackupmessagemap(orig, ui, repo):
    if commitcloudutil.getworkspacename(repo):
        return {
            'inprogress': 'syncing',
            'pending': 'sync pending',
            'failed':  'not synced',
        }
    else:
        return orig(ui, repo)

def _dobackgroundcloudsync(orig, ui, repo, dest=None, command=None):
    if commitcloudutil.getworkspacename(repo) is not None:
        return orig(ui, repo, dest, ['hg', 'cloud', 'sync'])
    else:
        return orig(ui, repo, dest, command)

def _smartlogbackupsuggestion(orig, ui, repo):
    if (commitcloudutil.getworkspacename(repo)):
        commitcloudcommon.highlightstatus(ui,
            _('Run `hg cloud sync` to synchronize your workspace. '
              'If this fails,\n'
              'please report to %s.\n') % commitcloudcommon.getownerteam(ui))
    else:
        orig(ui, repo)

def extsetup(ui):
    try:
        infinitepush = extensions.find('infinitepush')
    except KeyError:
        msg = _('The commitcloud extension requires the infinitepush extension')
        raise error.Abort(msg)
    extensions.wrapfunction(infinitepush.backupcommands, '_dobackgroundbackup',
                            _dobackgroundcloudsync)
    extensions.wrapfunction(infinitepush.backupcommands,
        '_smartlogbackupsuggestion', _smartlogbackupsuggestion)
    extensions.wrapfunction(infinitepush.backupcommands,
        '_smartlogbackupmessagemap', _smartlogbackupmessagemap)
    commitcloudcommands.infinitepush = infinitepush

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
