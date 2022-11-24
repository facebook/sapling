# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm import error
from edenscm.i18n import _

from .createremote import parselabels


def add_labels(ui, repo, csid=None, **opts):
    if csid is None:
        raise error.CommandError("snapshot add-labels", _("missing snapshot id"))
    labels = parselabels(opts)
    if not labels or len(labels) == 0:
        raise error.CommandError(
            "snapshot add-labels", _("missing labels to add to snapshot")
        )
    try:
        response = repo.edenapi.altersnapshot(
            {
                "cs_id": bytes.fromhex(csid),
                "labels_to_add": labels,
                "labels_to_remove": [],
            },
        )
    except Exception as e:
        ui.debug(f"error while adding labels: {e}\n")
        raise error.Abort(_("snapshot couldn't be updated\n"))
    current_labels = response["current_labels"]
    if current_labels:
        current_labels = ",".join(current_labels)
    ui.status(
        _("labels currently associated with snapshot: {}\n").format(current_labels)
    )


def remove_labels(ui, repo, csid=None, **opts):
    if csid is None:
        raise error.CommandError("snapshot remove-labels", _("missing snapshot id"))
    labels = parselabels(opts)
    if opts["all"] and labels and len(labels) > 0:
        raise error.CommandError(
            "snapshot remove-labels",
            _("cannot use 'labels' and 'all' arguments together"),
        )
    if not opts["all"] and (not labels or len(labels) == 0):
        raise error.CommandError(
            "snapshot remove-labels",
            _("need to provide atleast one of 'labels' or 'all' arguments"),
        )
    try:
        response = repo.edenapi.altersnapshot(
            {
                "cs_id": bytes.fromhex(csid),
                "labels_to_remove": labels if labels else [],
                "labels_to_add": [],
            },
        )
    except Exception as e:
        ui.debug(f"error while removing labels: {e}\n")
        raise error.Abort(_("snapshot couldn't be updated\n"))
    current_labels = response["current_labels"]
    if current_labels:
        current_labels = ",".join(current_labels)
    ui.status(
        _("labels currently associated with snapshot: {}\n").format(current_labels)
    )
