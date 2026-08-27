
#require no-eden

  $ eagerepo
`
  $ newext showhint << EOF
  > from sapling import (
  >     cmdutil,
  >     error,
  >     hintutil,
  >     registrar,
  > )
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > hint = registrar.hint()
  > 
  > @hint('next')
  > def hintnext(a, b):
  >     return "use 'sl next' to go from %s to %s" % (a, b)
  > 
  > @hint('export')
  > def hintexport(a):
  >     return "use 'sl export %s' to show commit content" % (a,)
  > 
  > @hint('slow')
  > def hintslow(a):
  >     return "%r is slow - be patient" % (a,)
  > 
  > @command('showhint', norepo=True)
  > def showhint(ui, *args):
  >     hintutil.trigger('export', 'P')
  >     hintutil.triggershow(ui, 'slow', 'date(x)')
  >     hintutil.trigger('next', 'X', 'Y')
  >     hintutil.trigger('export', 'Q')
  > 
  > @command('showhintabort', norepo=True)
  > def showhintabort(ui):
  >     hintutil.triggershow(ui, 'slow', 'date(x)')
  >     raise error.Abort('stopped after showing hint')
  > EOF

  $ sl showhint
  hint[slow]: 'date(x)' is slow - be patient
  hint[export]: use 'sl export P' to show commit content
  hint[next]: use 'sl next' to go from X to Y
  hint[hint-ack]: use 'sl hint --ack export next' to silence these hints

An aborted command does not suppress the same hint in the next command:

  $ sl showhintabort
  hint[slow]: 'date(x)' is slow - be patient
  abort: stopped after showing hint
  [255]
  $ sl showhint
  hint[slow]: 'date(x)' is slow - be patient
  hint[export]: use 'sl export P' to show commit content
  hint[next]: use 'sl next' to go from X to Y
  hint[hint-ack]: use 'sl hint --ack export next' to silence these hints

Test HGPLAIN=1 or HGPLAIN=hint silences all hints

  $ HGPLAIN=1 sl showhint
  $ HGPLAIN=hint sl showhint

Test silence configs

  $ sl showhint --config hint.ack-export=True --config hint.ack-slow=True
  hint[next]: use 'sl next' to go from X to Y
  hint[hint-ack]: use 'sl hint --ack next' to silence these hints
  $ sl showhint --config hint.ack=next
  hint[slow]: 'date(x)' is slow - be patient
  hint[export]: use 'sl export P' to show commit content
  hint[hint-ack]: use 'sl hint --ack export' to silence these hints
  $ sl showhint --config hint.ack=*

Test hint --ack command

  $ sl hint --ack next hint-ack
  hints about next, hint-ack are silenced
  $ sl config hint.ack
  smartlog-default-command commitcloud-update-on-move next hint-ack

  $ sl showhint
  hint[slow]: 'date(x)' is slow - be patient
  hint[export]: use 'sl export P' to show commit content

  $ sl hint --ack export slow -q
  $ sl config hint.ack
  smartlog-default-command commitcloud-update-on-move next hint-ack export slow

  $ sl showhint
