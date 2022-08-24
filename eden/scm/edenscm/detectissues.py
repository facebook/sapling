# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# detectissues.py - detect various issues with the repository


from __future__ import absolute_import

import os

from .i18n import _
from .pycompat import ossep


class issue(object):
    def __init__(self, category, message, data):
        self.category = category
        self.message = message
        self.data = data


def computecachesize(repo):
    """measure size of cache directory"""
    from .ext.remotefilelog import shallowutil

    cachepath = shallowutil.getcachepath(repo.ui)

    skipped = 0

    cachesize = 0
    manifestsize = 0
    for root, dirs, files in os.walk(cachepath):
        dirsize = 0
        for filename in files:
            try:
                stat = os.lstat(os.path.join(root, filename))
                dirsize += stat.st_size
            except Exception as e:
                repo.ui.warn(
                    _("error statting file '%s': %r. skipping file.\n") % (filename, e)
                )
                skipped += 1

        relpath = os.path.relpath(root, cachepath)
        segments = relpath.split(ossep)
        if "manifests" in segments[1:]:
            manifestsize += dirsize
        else:
            cachesize += dirsize

    return (cachesize, manifestsize, skipped)


def cachesizeexceedslimit(repo):
    cachelimit = repo.ui.configbytes("remotefilelog", "cachelimit", "10GB")
    manifestlimit = repo.ui.configbytes("remotefilelog", "manifestlimit", "2GB")
    cachesize, manifestsize, skipped = computecachesize(repo)
    issues = []
    if cachesize > cachelimit:
        issues.append(
            issue(
                "cache_size_exceeds_limit",
                _("cache size of %s exceeds configured limit of %s. %s files skipped.")
                % (cachesize, cachelimit, skipped),
                {
                    "cachesize": cachesize,
                    "manifestsize": manifestsize,
                    "cachelimit": cachelimit,
                    "manifestlimit": manifestlimit,
                    "skippedfiles": skipped,
                },
            )
        )
    if manifestsize > manifestlimit:
        issues.append(
            issue(
                "manifest_size_exceeds_limit",
                _(
                    "manifest cache size of %s exceeds configured limit of %s. %s files skipped."
                )
                % (manifestsize, manifestlimit, skipped),
                {
                    "cachesize": cachesize,
                    "manifestsize": manifestsize,
                    "cachelimit": cachelimit,
                    "manifestlimit": manifestlimit,
                    "skippedfiles": skipped,
                },
            )
        )
    return issues


def detectissues(repo):
    issuedetectors = [cachesizeexceedslimit]

    issues = {}
    for func in issuedetectors:
        name = func.__name__
        try:
            issues[name] = func(repo)
        except Exception as e:
            repo.ui.warn(
                _("exception %r while running issue detector %s, skipping\n")
                % (e, name)
            )

    return issues
