Output is normally only printed once
  $ hg dbsh -c 'ui.pushbuffer(error=True); ui.warn(_("testing!\n")); buffer = ui.popbuffer(); ui.warn(_(f"{buffer}"))'
  testing!

Teed output is printed twice
  $ hg dbsh -c 'ui.pushbuffer(error=True, tee=True); ui.warn(_("testing!\n")); buffer = ui.popbuffer(); ui.warn(_(f"{buffer}"))'
  testing!
  testing!
