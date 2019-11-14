Test the extensions.afterloaded() function

  $ cat > foo.py <<EOF
  > from edenscm.mercurial import extensions
  > def uisetup(ui):
  >     ui.write("foo.uisetup\\n")
  >     ui.flush()
  >     def bar_loaded(loaded):
  >         ui.write("foo: bar loaded: %r\\n" % (loaded,))
  >         ui.flush()
  >     extensions.afterloaded('bar', bar_loaded)
  > EOF
  $ cat > bar.py <<EOF
  > def uisetup(ui):
  >     ui.write("bar.uisetup\\n")
  >     ui.flush()
  > EOF
  $ basepath=`pwd`

  $ hg init basic
  $ cd basic
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> .hg/hgrc
  $ echo "foo = $basepath/foo.py" >> .hg/hgrc
  $ echo "bar = $basepath/bar.py" >> .hg/hgrc
  $ hg log -r. -T'{rev}\n'
  foo.uisetup
  foo: bar loaded: True
  bar.uisetup
  0

Test afterloaded with the opposite extension load order

  $ cd ..
  $ hg init basic_reverse
  $ cd basic_reverse
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> .hg/hgrc
  $ echo "bar = $basepath/bar.py" >> .hg/hgrc
  $ echo "foo = $basepath/foo.py" >> .hg/hgrc
  $ hg log -r. -T'{rev}\n'
  bar.uisetup
  foo.uisetup
  foo: bar loaded: True
  0

Test the extensions.afterloaded() function when the requested extension is not
loaded

  $ cd ..
  $ hg init notloaded
  $ cd notloaded
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> .hg/hgrc
  $ echo "foo = $basepath/foo.py" >> .hg/hgrc
  $ hg log -r. -T'{rev}\n'
  foo.uisetup
  foo: bar loaded: False
  0

Test the extensions.afterloaded() function when the requested extension is not
configured but fails the minimum version check

  $ cd ..
  $ cat > minvers.py <<EOF
  > minimumhgversion = '9999.9999'
  > def uisetup(ui):
  >     ui.write("minvers.uisetup\\n")
  >     ui.flush()
  > EOF
  $ hg init minversion
  $ cd minversion
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> .hg/hgrc
  $ echo "foo = $basepath/foo.py" >> .hg/hgrc
  $ echo "bar = $basepath/minvers.py" >> .hg/hgrc
  $ hg log -r. -T'{rev}\n'
  (third party extension bar requires version 9999.9999 or newer of Mercurial; disabling)
  foo.uisetup
  foo: bar loaded: False
  0

Test the extensions.afterloaded() function when the requested extension is not
configured but fails the minimum version check, using the opposite load order
for the two extensions.

  $ cd ..
  $ hg init minversion_reverse
  $ cd minversion_reverse
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> .hg/hgrc
  $ echo "bar = $basepath/minvers.py" >> .hg/hgrc
  $ echo "foo = $basepath/foo.py" >> .hg/hgrc
  $ hg log -r. -T'{rev}\n'
  (third party extension bar requires version 9999.9999 or newer of Mercurial; disabling)
  foo.uisetup
  foo: bar loaded: False
  0
