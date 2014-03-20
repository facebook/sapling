
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
  >          [('', 'param', [], 'stream level parameter'),
  >           ('', 'parts', False, 'include some arbitrary parts to the bundle'),],
  >          '[OUTPUTFILE]')
  > def cmdbundle2(ui, repo, path=None, **opts):
  >     """write a bundle2 container on standard ouput"""
  >     bundler = bundle2.bundle20(ui)
  >     for p in opts['param']:
  >         p = p.split('=', 1)
  >         try:
  >             bundler.addparam(*p)
  >         except ValueError, exc:
  >             raise util.Abort('%s' % exc)
  > 
  >     if opts['parts']:
  >        part = bundle2.part('test:empty')
  >        bundler.addpart(part)
  >        # add a second one to make sure we handle multiple parts
  >        part = bundle2.part('test:empty')
  >        bundler.addpart(part)
  > 
  >     if path is None:
  >        file = sys.stdout
  >     else:
  >         file = open(path, 'w')
  > 
  >     for chunk in bundler.getchunks():
  >         file.write(chunk)
  > 
  > @command('unbundle2', [], '')
  > def cmdunbundle2(ui, repo):
  >     """read a bundle2 container from standard input"""
  >     unbundler = bundle2.unbundle20(ui, sys.stdin)
  >     try:
  >         params = unbundler.params
  >     except KeyError, exc:
  >        raise util.Abort('unknown parameters: %s' % exc)
  >     ui.write('options count: %i\n' % len(params))
  >     for key in sorted(params):
  >         ui.write('- %s\n' % key)
  >         value = params[key]
  >         if value is not None:
  >             ui.write('    %s\n' % value)
  >     parts = list(unbundler)
  >     ui.write('parts count:   %i\n' % len(parts))
  >     for p in parts:
  >         ui.write('  :%s:\n' % p.type)
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

Test unknown mandatory option
---------------------------------------------------

  $ hg bundle2 --param 'Gravity' | hg unbundle2
  abort: unknown parameters: 'Gravity'
  [255]

Test debug output
---------------------------------------------------

bundling debug

  $ hg bundle2 --debug --param 'e|! 7/=babar%#==tutu' --param simple ../out.hg2
  start emission of HG20 stream
  bundle parameter: e%7C%21%207/=babar%25%23%3D%3Dtutu simple
  start of parts
  end of bundle

file content is ok

  $ cat ../out.hg2
  HG20\x00)e%7C%21%207/=babar%25%23%3D%3Dtutu simple\x00\x00 (no-eol) (esc)

unbundling debug

  $ hg unbundle2 --debug < ../out.hg2
  start processing of HG20 stream
  reading bundle2 stream parameters
  ignoring unknown parameter 'e|! 7/'
  ignoring unknown parameter 'simple'
  options count: 2
  - e|! 7/
      babar%#==tutu
  - simple
  start extraction of bundle2 parts
  part header size: 0
  end of bundle2 stream
  parts count:   0


Test buggy input
---------------------------------------------------

empty parameter name

  $ hg bundle2 --param '' --quiet
  abort: empty parameter name
  [255]

bad parameter name

  $ hg bundle2 --param 42babar
  abort: non letter first character: '42babar'
  [255]


Test part
=================

  $ hg bundle2 --parts ../parts.hg2 --debug
  start emission of HG20 stream
  bundle parameter: 
  start of parts
  bundle part: "test:empty"
  bundle part: "test:empty"
  end of bundle

  $ cat ../parts.hg2
  HG20\x00\x00\x00\r (esc)
  test:empty\x00\x00\x00\x00\x00\x00\x00\r (esc)
  test:empty\x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)


  $ hg unbundle2 < ../parts.hg2
  options count: 0
  parts count:   2
    :test:empty:
    :test:empty:

  $ hg unbundle2 --debug < ../parts.hg2
  start processing of HG20 stream
  reading bundle2 stream parameters
  options count: 0
  start extraction of bundle2 parts
  part header size: 13
  part type: "test:empty"
  part parameters: 0
  payload chunk size: 0
  part header size: 13
  part type: "test:empty"
  part parameters: 0
  payload chunk size: 0
  part header size: 0
  end of bundle2 stream
  parts count:   2
    :test:empty:
    :test:empty:
