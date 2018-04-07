  $ cat > showhint.py << EOF
  > from mercurial import (
  >     cmdutil,
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
  >     return "use 'hg next' to go from %s to %s" % (a, b)
  > 
  > @hint('export')
  > def hintexport(a):
  >     return "use 'hg export %s' to show commit content" % (a,)
  > 
  > @command('showhint', norepo=True)
  > def showhint(*args):
  >     hintutil.trigger('export', 'P')
  >     hintutil.trigger('next', 'X', 'Y')
  >     hintutil.trigger('export', 'Q')
  > EOF

  $ setconfig extensions.showhint=$TESTTMP/showhint.py

  $ hg showhint
  hint[export]: use 'hg export P' to show commit content
  hint[next]: use 'hg next' to go from X to Y

Test HGPLAIN=1 silences all hints

  $ HGPLAIN=1 hg showhint

Test silence configs

  $ hg showhint --config hint.ack-export=True
  hint[next]: use 'hg next' to go from X to Y
  $ hg showhint --config hint.ack=next
  hint[export]: use 'hg export P' to show commit content
  $ hg showhint --config hint.ack=*
