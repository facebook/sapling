"""enable ui.log output in tests

Wraps the ``ui.log`` method, printing out events which are enabled.

To enable events add them to the ``extralog.events`` config list.
"""


def uisetup(ui):
    class extralogui(ui.__class__):
        def log(self, event, *msg, **opts):
            items = self.configlist("extralog", "events")
            if event in items:
                if msg:
                    ui.write("%s: " % event)
                    ui.write(msg[0] % msg[1:])
                else:
                    ui.write("%s\n" % event)
            return super(extralogui, self).log(event, *msg, **opts)

    ui.__class__ = extralogui
