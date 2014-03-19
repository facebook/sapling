
Create an extension to test bundle2 API

  $ cat > bundle2.py << EOF
  > """A small extension to test bundle2 implementation
  > 
  > Current bundle2 implementation is far too limited to be used in any core
  > code. We still need to be able to test it while it grow up.
  > """
  > 
  > import sys
  > from mercurial import cmdutil
  > from mercurial import util
  > from mercurial import bundle2
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > @command('bundle2',
  >          [('', 'param', [], 'stream level parameter'),],
  >          '')
  > def cmdbundle2(ui, repo, **opts):
  >     """write a bundle2 container on standard ouput"""
  >     bundler = bundle2.bundle20()
  >     for p in opts['param']:
  >         p = p.split('=', 1)
  >         try:
  >             bundler.addparam(*p)
  >         except ValueError, exc:
  >             raise util.Abort('%s' % exc)
  > 
  >     for chunk in bundler.getchunks():
  >         ui.write(chunk)
  > 
  > @command('unbundle2', [], '')
  > def cmdunbundle2(ui, repo):
  >     """read a bundle2 container from standard input"""
  >     unbundler = bundle2.unbundle20(sys.stdin)
  >     ui.write('options count: %i\n' % len(unbundler.params))
  >     for key in sorted(unbundler.params):
  >         ui.write('- %s\n' % key)
  >         value = unbundler.params[key]
  >         if value is not None:
  >             ui.write('    %s\n' % value)
  >     parts = list(unbundler)
  >     ui.write('parts count:   %i\n' % len(parts))
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > bundle2=$TESTTMP/bundle2.py
  > EOF

The extension requires a repo (currently unused)

  $ hg init main
  $ cd main
  $ touch a
  $ hg add a
  $ hg commit -m 'a'


Empty bundle
=================

- no option
- no parts

Test bundling

  $ hg bundle2
  HG20\x00\x00\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 | hg unbundle2
  options count: 0
  parts count:   0

Test old style bundle are detected and refused

  $ hg bundle --all ../bundle.hg
  1 changesets found
  $ hg unbundle2 < ../bundle.hg
  abort: unknown bundle version 10
  [255]

Test parameters
=================

- some options
- no parts

advisory parameters, no value
-------------------------------

Simplest possible parameters form

Test generation simple option

  $ hg bundle2 --param 'caution'
  HG20\x00\x07caution\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'caution' | hg unbundle2
  options count: 1
  - caution
  parts count:   0

Test generation multiple option

  $ hg bundle2 --param 'caution' --param 'meal'
  HG20\x00\x0ccaution meal\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'caution' --param 'meal' | hg unbundle2
  options count: 2
  - caution
  - meal
  parts count:   0

advisory parameters, with value
-------------------------------

Test generation

  $ hg bundle2 --param 'caution' --param 'meal=vegan' --param 'elephants'
  HG20\x00\x1ccaution meal=vegan elephants\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'caution' --param 'meal=vegan' --param 'elephants' | hg unbundle2
  options count: 3
  - caution
  - elephants
  - meal
      vegan
  parts count:   0

parameter with special char in value
---------------------------------------------------

Test generation

  $ hg bundle2 --param 'e|! 7/=babar%#==tutu' --param simple
  HG20\x00)e%7C%21%207/=babar%25%23%3D%3Dtutu simple\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'e|! 7/=babar%#==tutu' --param simple | hg unbundle2
  options count: 2
  - e|! 7/
      babar%#==tutu
  - simple
  parts count:   0

Test buggy input
---------------------------------------------------

empty parameter name

  $ hg bundle2 --param '' --quiet
  abort: empty parameter name
  [255]
