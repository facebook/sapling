# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Facilities to record pushrebase traffic. Primary motivation is to replay it
# on another hg backend e. g. Mononoke

import json
import subprocess
import time

from edenscm.mercurial.i18n import _
from edenscm.mercurial.node import hex

from ..extlib import mysqlutil


def recordpushrebaserequest(repo, conflicts, pushrebaseerrmsg, starttime):
    """Uploads bundle parts to a bundlestore, and then inserts the parameters
    in the mysql table
    """

    if not repo.ui.configbool("pushrebase", "enablerecording"):
        return

    # pushrebaserecordingparams may not be set if pushrebase hooks failed
    # or if it was non-pushrebase push (i.e. commit cloud push)
    if not getattr(repo, "pushrebaserecordingparams", None):
        return

    try:
        return _dorecordpushrebaserequest(repo, conflicts, pushrebaseerrmsg, starttime)
    except Exception as ex:
        # There is no need to fail the push, but at least let's log
        # the problem
        repo.ui.warn(_("error while recording pushrebase request %s") % ex)


def _dorecordpushrebaserequest(repo, conflicts, pushrebaseerrmsg, starttime):
    durationms = (time.time() - starttime) * 1000
    # Limit pushrebaseerrmsg to protect from too long messages
    if pushrebaseerrmsg:
        pushrebaseerrmsg = pushrebaseerrmsg[:1000]

    logparams = {
        "conflicts": conflicts,
        "pushrebase_errmsg": pushrebaseerrmsg,
        "duration_ms": durationms,
    }
    uploaderrmsg = None

    # Upload bundle to the remote storage, and collect the handles.
    returncode, stdout, stderr = _uploadtobundlestore(repo.ui, repo.unbundlefile)
    if returncode == 0:
        logparams["bundlehandle"] = stdout
    else:
        uploaderrmsg = "failed to upload: %s %s" % (stdout, stderr)

    logparams["upload_errmsg"] = uploaderrmsg

    # Collect the rest of the parameters.
    logparams.update(repo.pushrebaserecordingparams)

    # Collect timestamps and manifest hashes (but only if there were no errors)
    if not pushrebaseerrmsg and getattr(repo, "pushrebaseaddedchangesets", None):
        # We want to record mappings from old commit hashes to timestamps
        # of new commits and manifest hashes of new commits. To do this, we
        # need to revert replacements dict
        reversedreplacements = {}
        for oldrev, newrev in repo.pushrebasereplacements.items():
            reversedreplacements[newrev] = oldrev

        timestamps = {}
        manifests = {}
        for binaddednode in repo.pushrebaseaddedchangesets:
            if hex(binaddednode) in reversedreplacements:
                hexoldnode = reversedreplacements[hex(binaddednode)]
                if binaddednode in repo:
                    ctx = repo[binaddednode]
                    timestamps[hexoldnode] = ctx.date()
                    manifests[hexoldnode] = hex(ctx.manifestnode())
        logparams["timestamps"] = json.dumps(timestamps)
        logparams["recorded_manifest_hashes"] = json.dumps(manifests)

    # Insert into mysql table
    _recordpushrebaserecordtodb(repo.ui, logparams)


def _uploadtobundlestore(ui, file):
    writeargs = ui.configlist("pushrebase", "bundlepartuploadbinary", [])
    args = [arg.format(filename=file) for arg in writeargs]
    p = subprocess.Popen(
        args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, close_fds=True
    )
    stdout, stderr = p.communicate()
    returncode = p.returncode
    return returncode, stdout, stderr


def _recordpushrebaserecordtodb(ui, params):
    errmsg = "pushrebase request was not recorded"
    try:
        import mysql.connector
    except ImportError:
        ui.warn(_("%s: %s\n") % (errmsg, "mysql connector is not installed"))
        return

    repoid = ui.configint("pushrebase", "recordingrepoid")
    params["repo_id"] = repoid
    errmsg = "pushrebase was not recorded"
    if repoid is None:
        ui.warn(_("%s: %s") % (errmsg, "recordingrepoid was not set"))
        return

    sqlargs = ui.config("pushrebase", "recordingsqlargs")
    if not sqlargs:
        ui.warn(_("%s: %s") % (errmsg, "recordingsqlargs were not set"))
        return

    try:
        sqlargs = mysqlutil.parseconnectionstring(sqlargs)
    except mysqlutil.InvalidConnectionString:
        ui.warn(_("%s: %s") % (errmsg, "recordingsqlargs are invalid"))
        return

    sqlconn = mysql.connector.connect(force_ipv6=True, **sqlargs)
    mysqlutil.insert(sqlconn, "pushrebaserecording", params)
