# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""replace terms with more widely used equivalents

With this extension enabled, some terms will be replaced by their more
well-known equivalents. Namely, "changeset" will be replaced by "commit".
"""

testedwith = "ships-with-fb-ext"

from edenscm import extensions, i18n


def _ugettext(orig, message):
    if orig:
        message = orig(message)
    message = message.replace("changeset", "commit")
    return message


def _ungettext(orig, singular, plural, count):
    if orig:
        message = orig(singular, plural, count)
    message = message.replace("changeset", "commit")
    return message


def uisetup(ui):
    extensions.wrapfunction(i18n, "_ugettext", _ugettext)
    extensions.wrapfunction(i18n, "_ungettext", _ungettext)
