# embeddedimport - an ability to load python modules from a zip file
#
# Copyright 2018 Facebook Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#
# This module, together some other logic in Mercurial, mimics the approach
# of `py2exe`, albeit does it simpler. Basically, standard `zipimport` module
# already knows how to load Python modules from a `.zip` file, but it cannot
# load the dynamic library extensions from a zip file. This may be a problem
# when those extensions are a part of a package together with some Python
# modules. Here we teach Python to load such extensions from files
# named `path.to.packaged.module[pyd|so]` when the code contains
# `import path.to.packaged.module`.

import imp
import os
import sys
import zipimport


class EmbeddedImporter(zipimport.zipimporter):
    """ZipImporter with the ability to load .pyd/.so extensions

    We cannot use separate importers to load from .zip file and to
    load .pyd/.so extensions, because Python caches importers and
    once a package is being imported by some importer, all of the
    modules form this package will be imported by the same one.
    Because `mercurial` is a package, which contains both Python
    and native modules, we need to make sure that the same importer
    can deal with both types.
    Note also that this class is intantiated by the runtime,
    not by us and we don't control what's passed to __init__,
    so we can't make instance-level parametrization."""

    # directories where native modules should be sought
    nativeextdirs = []
    # File extensions that indicate shared-lib-based Python extension
    suffixes = [s[0] for s in imp.get_suffixes() if s[2] == imp.C_EXTENSION]

    def find_module(self, fullname, path=None):
        result = zipimport.zipimporter.find_module(self, fullname, path)
        if result:
            return result
        for nativextdir in self.nativeextdirs:
            fullname = os.path.join(nativextdir, fullname)
            for suff in self.suffixes:
                if os.path.exists(fullname + suff):
                    return self
        return None

    def load_module(self, fullname):
        if fullname in sys.modules:
            return sys.modules[fullname]
        try:
            result = zipimport.zipimporter.load_module(self, fullname)
        except zipimport.ZipImportError:
            result = None
        if result:
            return result
        modname = fullname
        for nativextdir in self.nativeextdirs:
            fullname = os.path.join(nativextdir, fullname)
            for suff in self.suffixes:
                if os.path.exists(fullname + suff):
                    return imp.load_dynamic(modname, fullname + suff)
        return None


def tryenableembedded():
    """Enable the embedded-extension-loading hook if we are in the zipfile"""
    dirname = os.path.dirname
    nativeextdirs = []
    for item in sys.path:
        item = os.path.realpath(item)
        while not (item.endswith(".zip") and os.path.isfile(item)):
            dn = dirname(item)
            if dn == item:
                break
            item = dn
        # if dirname(item) == item, it is guranteed to be a drive/fs root
        # so it's enough to just check that item.endswith(".zip")
        if item.endswith(".zip"):
            nativeextdirs.append(dirname(item))

    if not nativeextdirs:
        return

    EmbeddedImporter.nativeextdirs = list(set(nativeextdirs))
    new_hooks = [EmbeddedImporter]
    new_hooks.extend([ph for ph in sys.path_hooks if ph is not zipimport.zipimporter])
    sys.path_hooks = new_hooks
    # at this time the default (not modified) zipimporter is already
    # cached, so let's invalidate the caches
    sys.path_importer_cache.clear()
