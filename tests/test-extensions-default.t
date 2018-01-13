Tests the behavior of the DEFAULT_EXTENSIONS constant in extensions.py

  $ hg init

hg githelp works without enabling:

  $ hg githelp -- git reset HEAD
  hg reset .

Behaves identically if enabled manually:

  $ hg githelp --config extensions.githelp= -- git reset HEAD
  hg reset .

Not if turned off:

  $ hg githelp --config extensions.githelp=! -- git reset HEAD
  hg: unknown command 'githelp'
  'githelp' is provided by the following extension:
  
      githelp       try mapping git commands to Mercurial commands
  
  (use 'hg help extensions' for information on enabling extensions)
  [255]

Or overriden by a different path:

  $ cat > githelp2.py <<EOF
  > from __future__ import absolute_import
  > from mercurial import registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command('githelp')
  > def githhelp(ui, repo, *args, **opts):
  >      ui.warn('Custom version of hg githelp')
  > 
  > EOF
  $ hg githelp --config extensions.githelp=`pwd`/githelp2.py -- git reset HEAD
  Custom version of hg githelp (no-eol)
