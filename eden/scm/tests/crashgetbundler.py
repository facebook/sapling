from __future__ import absolute_import

from edenscm import changegroup, error, extensions
from edenscm.i18n import _


def abort(orig, *args, **kwargs):
    raise error.Abort(_("this is an exercise"))


def uisetup(ui):
    extensions.wrapfunction(changegroup, "getbundler", abort)
