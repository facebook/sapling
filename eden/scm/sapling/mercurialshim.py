# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import importlib
import importlib.abc
import importlib.util
import types


class MercurialImporter(importlib.abc.Loader, importlib.abc.MetaPathFinder):
    """
    Intercept legacy imports using "edenscm", "mercurial", or "edenscm.mercurial" and
    resolve with import of sapling$1.

    This provides compatibility to legacy hooks and extensions that still use old imports.
    """

    legacy_names = sorted(
        ["edenscm.mercurial", "mercurial", "edenscm"], key=len, reverse=True
    )
    legacy_names_set = set(legacy_names)
    legacy_prefixes = tuple(ln + "." for ln in legacy_names)

    def find_spec(
        self, fullname: str, path=None, target=None
    ) -> importlib.machinery.ModuleSpec | None:
        if fullname in self.legacy_names_set or fullname.startswith(
            self.legacy_prefixes
        ):
            return importlib.util.spec_from_loader(fullname, self)
        return None

    def create_module(self, spec: importlib.machinery.ModuleSpec) -> types.ModuleType:
        name = spec.name
        for ln in self.legacy_names:
            suffix = name.removeprefix(ln)
            if suffix != name:
                return importlib.import_module("sapling" + suffix)

        raise ImportError(f"MercurialImporter used for non-legacy module {name}")

    def exec_module(self, module):
        # These modules are already executed. This is a no-op.
        pass
