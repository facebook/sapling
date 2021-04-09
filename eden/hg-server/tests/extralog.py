"""enable ui.log output in tests

Wraps the ``ui.log`` method, printing out events which are enabled.

To enable events add them to the ``extralog.events`` config list.
"""

from __future__ import absolute_import

from edenscm.mercurial import extensions, util


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


def uisetup(ui):
    class extralogui(ui.__class__):
        def log(self, event, *msg, **opts):
            logevent(self, event, *msg, **opts)
            return super(extralogui, self).log(event, *msg, **opts)

    ui.__class__ = extralogui

    # Wrap util.log as an inner function so that we can use the ui object.
    def utillog(orig, event, *msg, **opts):
        logevent(ui, event, *msg, **opts)
        return orig(event, *msg, **opts)

    extensions.wrapfunction(util, "log", utillog)
