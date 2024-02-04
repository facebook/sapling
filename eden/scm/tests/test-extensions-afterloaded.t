#debugruntest-compatible
#inprocess-hg-incompatible

  $ eagerepo
Test the extensions.afterloaded() function

  $ cat > foo.py <<EOF
  > from sapling import extensions
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
  $ hg log -r. -T'{node}\n'
  foo.uisetup
  foo: bar loaded: True
  bar.uisetup
  c24b9ac61126c9cd86d5d684f8408cdc717005a4

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
  $ hg log -r. -T'{node}\n'
  bar.uisetup
  foo.uisetup
  foo: bar loaded: True
  c24b9ac61126c9cd86d5d684f8408cdc717005a4

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
  $ hg log -r. -T'{node}\n'
  foo.uisetup
  foo: bar loaded: False
  c24b9ac61126c9cd86d5d684f8408cdc717005a4
