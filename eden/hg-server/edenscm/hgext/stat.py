# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from edenscm.mercurial import error, patch, registrar, templatekw, util
from edenscm.mercurial.i18n import _


templatefunc = registrar.templatefunc()


@templatefunc("stat(style=none)", argspec="style")
def showdiffstat(context, mapping, args):
    """String. Return diffstat-style summary of changes.

    If 'style' is not 'none', it could be 'status', in which case "added",
    "changed", "removed" will be shown before file names.
    """
    if "style" in args:
        style = args["style"][1]
    else:
        style = "none"

    repo = mapping["repo"]
    ctx = mapping["ctx"]
    revcache = mapping["revcache"]
    width = repo.ui.termwidth()

    if style == "none":
        status = None
    elif style == "status":
        status = templatekw.getfiles(repo, ctx, revcache)
    else:
        raise error.ParseError(_("stat does not support style %r") % (style,))

    return patch.diffstat(
        util.iterlines(ctx.diff(noprefix=False)), width=width, status=status
    )
