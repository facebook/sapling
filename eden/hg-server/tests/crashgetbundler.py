from __future__ import absolute_import

from edenscm.mercurial import changegroup, error, extensions
from edenscm.mercurial.i18n import _


def abort(orig, *args, **kwargs):
    raise error.Abort(_("this is an exercise"))


def uisetup(ui):
    extensions.wrapfunction(changegroup, "getbundler", abort)
