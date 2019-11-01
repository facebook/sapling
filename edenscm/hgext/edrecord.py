# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import cmdutil, crecord as crecordmod, patch as patchmod, util
from edenscm.mercurial.i18n import _


testedwith = "ships-with-fb-hgext"

stringio = util.stringio
originaldorecord = cmdutil.dorecord
originalrecordfilter = cmdutil.recordfilter


def uisetup(ui):
    # "editor" is otherwise not allowed as a valid option for "ui.interface"
    class edrecordui(ui.__class__):
        def interface(self, feature):
            if feature == "chunkselector":
                configvalue = self.config("ui", "interface.%s" % feature)
                if configvalue == "editor":
                    return "editor"
                elif configvalue is None:
                    if self.config("ui", "interface") == "editor":
                        return "editor"
            return super(edrecordui, self).interface(feature)

    ui.__class__ = edrecordui
    cmdutil.recordfilter = recordfilter
    cmdutil.dorecord = dorecord


def dorecord(ui, repo, commitfunc, cmdsuggest, backupall, filterfn, *pats, **opts):
    if ui.interface("chunkselector") != "editor":
        return originaldorecord(
            ui, repo, commitfunc, cmdsuggest, backupall, filterfn, *pats, **opts
        )

    overrides = {("ui", "interactive"): True}
    with ui.configoverride(overrides, "edrecord"):
        originaldorecord(
            ui, repo, commitfunc, cmdsuggest, backupall, filterfn, *pats, **opts
        )


def recordfilter(ui, headers, operation=None):

    if ui.interface("chunkselector") != "editor":
        return originalrecordfilter(ui, headers, operation)

    # construct diff string from headers
    if len(headers) == 0:
        return [], {}

    patch = stringio()
    patch.write(crecordmod.diffhelptext)

    specials = {}

    for header in headers:
        patch.write("#\n")
        if header.special():
            # this is useful for special changes, we are able to get away with
            # only including the parts of headers that offer useful info
            specials[header.filename()] = header
            for h in header.header:
                if h.startswith("index "):
                    # starting at 'index', the headers for binary files tend to
                    # stop offering useful info for the viewer
                    patch.write(
                        _(
                            """\
# this modifies a binary file (all or nothing)
"""
                        )
                    )
                    break
                if not h.startswith("diff "):
                    # For specials, we only care about the filename header.
                    # The rest can be displayed as comments
                    patch.write("# ")
                patch.write(h)
        else:
            header.write(patch)
            for hunk in header.hunks:
                hunk.write(patch)

    patcheditor = ui.config("ui", "editor.chunkselector")
    if patcheditor is not None:
        override = {("ui", "editor"): patcheditor}
    else:
        override = {}

    with ui.configoverride(override):
        patch = ui.edit(patch.getvalue(), "", action=(operation or "edit"))

    # remove comments from patch
    # if there's an empty line, add a space to it
    patch = [
        (line if len(line) > 0 else " ") + "\n"
        for line in patch.splitlines()
        if not line.startswith("#")
    ]

    headers = patchmod.parsepatch(patch)

    applied = {}
    for h in headers:
        if h.filename() in specials:
            h = specials[h.filename()]
        applied[h.filename()] = [h] + h.hunks

    return (
        sum([i for i in applied.itervalues() if i[0].special() or len(i) > 1], []),
        {},
    )
