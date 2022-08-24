"""enable ui.log output in tests

Wraps the ``ui.log`` method, printing out events which are enabled.

To enable events add them to the ``extralog.events`` config list.
"""

from __future__ import absolute_import

from edenscm import extensions, util


def logevent(ui, event, *msg, **opts):
    items = ui.configlist("extralog", "events")
    if event in items:
        keywords = ""
        if opts and ui.configbool("extralog", "keywords"):
            keywords = " (%s)\n" % " ".join(
                "%s=%s" % (n, v) for n, v in sorted(opts.items())
            )
        if msg:
            ui.write("%s: " % event)
            ui.write(msg[0] % msg[1:])
            ui.write("%s" % keywords)
        else:
            ui.write("%s%s" % (event, keywords))


class uilogmixin:
    def log(self, event, *msg, **opts):
        logevent(self, event, *msg, **opts)
        return super(uilogmixin, self).log(event, *msg, **opts)


loguis = []


def reposetup(ui, repo):
    if uilogmixin not in ui.__class__.mro():

        class extralogui(uilogmixin, ui.__class__):
            pass

        ui.__class__ = extralogui
        loguis.append(ui)


def uisetup(ui):
    # Wrap util.log as an inner function so that we can use the ui object.
    def utillog(orig, event, *msg, **opts):
        for ui in loguis:
            logevent(ui, event, *msg, **opts)
        return orig(event, *msg, **opts)

    extensions.wrapfunction(util, "log", utillog)
