# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


from sapling import edenapi_upload, git, node as nodemod
from sapling.i18n import _, _n


def upload(repo, revs, force=False, localbackupstate=None):
    """Upload draft commits using EdenApi Uploads

    Commits that have already been uploaded will be skipped.
    If no revision is specified, uploads all visible commits.

    If localbackupstate is provided, it will be updated during the upload.

    Returns list of uploaded heads (as nodes) and list of failed commits (as nodes).
    """
    ui = repo.ui

    if revs is None:
        heads = [ctx.node() for ctx in repo.set("heads(not public())")]
    else:
        heads = [
            ctx.node()
            for ctx in repo.set(
                "heads((not public() & ::%ld))",
                revs,
            )
        ]
    if not heads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return [], []

    maybemissingheads = heads

    if localbackupstate and not force:
        # Filter heads that are known to be backed (check local backup cache)
        maybemissingheads = localbackupstate.filterheads(heads)

    if not maybemissingheads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        return heads, []

    # Check with the server what heads have been already uploaded and what heads are missing
    missingheads = (
        maybemissingheads
        if force
        else edenapi_upload._filtercommits(repo, maybemissingheads)
    )

    if not missingheads:
        ui.status(_("nothing to upload\n"), component="commitcloud")
        if localbackupstate:
            localbackupstate.update(heads)
        return heads, []

    # Print the heads missing on the server
    _maxoutput = 20
    for counter, node in enumerate(missingheads):
        if counter == _maxoutput:
            left = len(missingheads) - counter
            repo.ui.status(
                _n(
                    "  and %d more head...\n",
                    "  and %d more heads...\n",
                    left,
                )
                % left
            )
            break
        ui.status(
            _("head '%s' hasn't been uploaded yet\n") % nodemod.hex(node)[:12],
            component="commitcloud",
        )

    if git.isgitformat(repo):
        # Use `git push` to upload the commits.
        pairs = [
            (h, f"{git.COMMIT_CLOUD_UPLOAD_REF}{i}")
            for i, h in enumerate(sorted(missingheads))
        ]
        ret = git.push(repo, "default", pairs)
        if ret == 0:
            newuploaded, failednodes = missingheads, []
        else:
            newuploaded, failednodes = [], missingheads
    else:
        draftnodes = list(repo.dageval(lambda: draft() & ancestors(missingheads)))

        # If the only draft nodes are the missing heads then we can skip the
        # known checks, as we know they are all missing.
        skipknowncheck = len(draftnodes) == len(missingheads)
        newuploaded, failednodes = edenapi_upload.uploadhgchangesets(
            repo, draftnodes, force, skipknowncheck
        )

    # Uploaded heads are all heads that have been filtered or uploaded and also heads of the 'newuploaded' nodes.

    # Example (5e4faf031 must be included in uploadedheads):
    #  o  4bb40f883 (failed)
    #  │
    #  @  5e4faf031 (uploaded)

    uploadedheads = list(
        repo.nodes("heads(%ln) + %ln - heads(%ln)", newuploaded, heads, failednodes)
    )

    if localbackupstate:
        localbackupstate.update(uploadedheads)

    return uploadedheads, failednodes
