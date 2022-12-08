# Portions Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Copyright 2009, Alexander Solovyov <piranha@piranha.org.ua>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

"""extend schemes with shortcuts to repository swarms

This extension allows you to specify shortcuts for parent URLs with a
lot of repositories to act like a scheme, for example::

  [schemes]
  py = http://code.python.org/hg/

After that you can use it like::

  hg clone py://trunk/

If the scheme URL is not to append at the end, use '{1}' to specify its
location. For example::

  [schemes]
  gcode = http://{1}.googlecode.com/hg/

Note only '{1}' is supported. There is no '{2}', '{0}' or other template
functions.
"""
from __future__ import absolute_import

import os

from edenscm import error, extensions, hg, pycompat, registrar, ui as uimod, util
from edenscm.i18n import _

cmdtable = {}
command = registrar.command(cmdtable)
# Note for extension authors: ONLY specify testedwith = 'ships-with-hg-core' for
# extensions which SHIP WITH MERCURIAL. Non-mainline extensions should
# be specifying the version(s) of Mercurial they are tested with, or
# leave the attribute unspecified.
testedwith = b"ships-with-hg-core"


class ShortRepository(object):
    def __init__(self, url, scheme):
        self.scheme = scheme
        self.url = url

    def __repr__(self):
        return "<ShortRepository: %s>" % self.scheme

    def instance(self, ui, url, create, initial_config):
        url = self.resolve(url)
        return hg._peerlookup(url).instance(ui, url, create, initial_config)

    def resolve(self, url):
        # We are only interested in the part after the scheme, so split on the
        # first colon, and discard any leading slashes.
        try:
            scheme_specific_part = url.split(":", 1)[1].lstrip("/")
        except IndexError:
            raise error.Abort(_("no ':' in url '%s'") % url)
        placeholder = "{1}"
        if placeholder in self.url:
            return self.url.replace(placeholder, scheme_specific_part)
        else:
            return self.url + scheme_specific_part


def hasdriveletter(orig, path):
    if path:
        for scheme in schemes:
            if path.startswith(scheme + ":"):
                return False
    return orig(path)


def normalizepath(orig, path):
    if path:
        for scheme in schemes:
            if path.startswith(scheme + ":"):
                repo = hg._peerlookup(path)
                if isinstance(repo, ShortRepository):
                    path = repo.resolve(path)
    return orig(path)


schemes = {}


def extsetup(ui):
    schemes.update(dict(ui.configitems("schemes")))
    for scheme, url in schemes.items():
        if (
            pycompat.iswindows
            and len(scheme) == 1
            and scheme.isalpha()
            and os.path.exists("%s:\\" % scheme)
        ):
            raise error.Abort(
                _("custom scheme %s:// conflicts with drive letter %s:\\\n")
                % (scheme, scheme.upper())
            )
        hg.schemes[scheme] = ShortRepository(url, scheme)

    extensions.wrapfunction(util, "hasdriveletter", hasdriveletter)
    extensions.wrapfunction(uimod, "_normalizepath", normalizepath)


@command("debugexpandscheme", norepo=True)
def expandscheme(ui, url, **opts):
    """given a repo path, provide the scheme-expanded path"""
    repo = hg._peerlookup(url)
    if isinstance(repo, ShortRepository):
        url = repo.resolve(url)
    ui.write(url + "\n")


@command("debugexpandpaths")
def expandschemes(ui, repo, *args, **opts):
    """given a repo path, provide the scheme-expanded path"""
    for name, path in sorted(pycompat.iteritems(ui.paths)):
        url = path.rawloc
        repo = hg._peerlookup(url)

        debugstatus = " (not expanded)"
        if isinstance(repo, ShortRepository):
            debugstatus = " (expanded from " + url + ")"
            url = repo.resolve(url)
        ui.write(_("paths." + name + "=" + url + debugstatus + "\n"))
