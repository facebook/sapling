#require fsmonitor

  $ setconfig workingcopy.ruststatus=False
  $ setconfig fsmonitor.detectrace=1
  $ newrepo

No races for common operations

  $ touch x
  $ hg status
  ? x

  $ rm x
  $ touch y
  $ hg status
  ? y

Create a race by write files by writing files if context._dirstatestatus is called

  $ echo 'f' > .gitignore
  $ mkdir c
  $ touch e f g
  $ cat > $TESTTMP/racy.py << EOF
  > from edenscm.hgext import fsmonitor
  > from edenscm.mercurial import extensions
  > def _race(orig, *args, **kwargs):
  >     open('a', 'w').close()
  >     open('f', 'w').close()
  >     open('c/d.txt', 'w').close()
  >     return orig(*args, **kwargs)
  > def uisetup(ui):
  >     extensions.wrapfunction(fsmonitor, "_walk", _race)
  > EOF

  $ hg status --config extensions.racy=$TESTTMP/racy.py
  abort: [race-detector] files changed when scanning changes in working copy:
    a
    c/d.txt
  
  (this is an error because HGDETECTRACE or fsmonitor.detectrace is set to true)
  [75]

The race detector does not check files outside specified patterns:

  $ hg status --config extensions.racy=$TESTTMP/racy.py y
  ? y

  $ hg status --config extensions.racy=$TESTTMP/racy.py y a
  abort: [race-detector] files changed when scanning changes in working copy:
    a
  
  (this is an error because HGDETECTRACE or fsmonitor.detectrace is set to true)
  [75]

Ignored files do not use fsmonitor. So race detector does not detect it:

  $ hg status -i --config extensions.racy=$TESTTMP/racy.py
  I f

Race detector can be turned off:

  $ hg status --config extensions.racy=$TESTTMP/racy.py --config fsmonitor.detectrace=0
  ? .gitignore
  ? a
  ? c/d.txt
  ? e
  ? g
  ? y
