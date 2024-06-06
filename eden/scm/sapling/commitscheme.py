# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import re
from typing import List, Optional

from sapling import git, util

from sapling.i18n import _


class schemes:
    """schemes is a utility class for managing the translation between
    different commit schemes (e.g. hg <-> bonsai). It reads scheme
    rules out of the config, and currently only uses EdenApi to
    perform the translation."""

    def __init__(self, repo):
        self._ui = repo.ui
        self._schemes = []
        for key, value in self._ui.configitems("commit-scheme"):
            parts = key.split(".", 1)
            if len(parts) != 2:
                continue

            if parts[1] == "re":
                self._schemes.append((_normalize_scheme(parts[0]), re.compile(value)))

        self._cache = util.lrucachedict(100)
        self._edenapi = repo.nullableedenapi

        if git.isgitformat(repo):
            self._local_scheme = "git"
        else:
            self._local_scheme = "hg"

    def possible_schemes(self, commit_id: str) -> List[str]:
        """Evaluate commit scheme rules yielding a list of commit
        scheme that commit_id might belong to."""
        matches = []
        for scheme in self._schemes:
            if scheme[1].match(commit_id):
                matches.append(scheme[0])
        return matches

    def translate(self, commit_id, to_scheme, from_scheme=None) -> Optional[str]:
        """Translate commit_id to to_scheme. If from_scheme is not
        provided, possible_schemes() will be used to guess the scheme.
        to_scheme or from_scheme can be set to "local", which means
        use the local repo's commit scheme."""
        if not self._edenapi:
            return None

        if from_scheme == "local":
            from_scheme = self._local_scheme
        if to_scheme == "local":
            to_scheme = self._local_scheme

        to_scheme = _normalize_scheme(to_scheme)

        if cached := self._cache.get((commit_id, to_scheme)):
            return cached

        from_scheme = _normalize_scheme(from_scheme)

        if from_scheme is not None:
            from_schemes = [from_scheme]
        else:
            from_schemes = self.possible_schemes(commit_id)

        translated = None

        for from_scheme in from_schemes:
            # We may want to cache this remote call.
            try:
                resp = list(
                    self._edenapi.committranslateids(
                        [{from_scheme: commit_id}], to_scheme
                    )
                )
            except Exception as e:
                self._ui.warn(
                    _("error translating %s from %s to %s: %s\n")
                    % (commit_id, from_scheme, to_scheme, e)
                )
                continue

            if len(resp) == 1:
                translated = resp[0]["translated"][to_scheme]
                break

        self._cache[(commit_id, to_scheme)] = translated

        return translated


# Offer more user friendly names than our edenapi constants.
_scheme_mappings = {
    "hg": "Hg",
    "bonsai": "Bonsai",
    "git": "GitSha1",
    "globalrev": "Globalrev",
}


def _normalize_scheme(name: Optional[str]) -> Optional[str]:
    if name is None:
        return None

    return _scheme_mappings.get(name, name)
