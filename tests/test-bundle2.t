
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
  > ELEPHANTSSONG = """Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
  > Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  > Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko."""
  > assert len(ELEPHANTSSONG) == 178 # future test say 178 bytes, trust it.
  > 
  > @bundle2.parthandler('test:song')
  > def songhandler(repo, part):
  >     """handle a "test:song" bundle2 part, printing the lyrics on stdin"""
  >     repo.ui.write('The choir start singing:\n')
  >     for line in part.data.split('\n'):
  >         repo.ui.write('    %s\n' % line)
  > 
  > @command('bundle2',
  >          [('', 'param', [], 'stream level parameter'),
  >           ('', 'unknown', False, 'include an unknown mandatory part in the bundle'),
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
  >        part = bundle2.part('test:song', data=ELEPHANTSSONG)
  >        bundler.addpart(part)
  >        part = bundle2.part('test:math',
  >                            [('pi', '3.14'), ('e', '2.72')],
  >                            [('cooking', 'raw')],
  >                            '42')
  >        bundler.addpart(part)
  >     if opts['unknown']:
  >        part = bundle2.part('test:UNKNOWN',
  >                            data='some random content')
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
  >     """process a bundle2 stream from stdin on the current repo"""
  >     try:
  >         lock = repo.lock()
  >         try:
  >             bundle2.processbundle(repo, sys.stdin)
  >         except KeyError, exc:
  >             raise util.Abort('missing support for %s' % exc)
  >     finally:
  >         lock.release()
  >         remains = sys.stdin.read()
  >         ui.write('%i unread bytes\n' % len(remains))
  > 
  > @command('statbundle2', [], '')
  > def cmdstatbundle2(ui, repo):
  >     """print statistic on the bundle2 container read from stdin"""
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
  >         ui.write('    mandatory: %i\n' % len(p.mandatoryparams))
  >         ui.write('    advisory: %i\n' % len(p.advisoryparams))
  >         ui.write('    payload: %i bytes\n' % len(p.data))
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

  $ hg bundle2 | hg statbundle2
  options count: 0
  parts count:   0

Test old style bundle are detected and refused

  $ hg bundle --all ../bundle.hg
  1 changesets found
  $ hg statbundle2 < ../bundle.hg
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

  $ hg bundle2 --param 'caution' | hg statbundle2
  options count: 1
  - caution
  parts count:   0

Test generation multiple option

  $ hg bundle2 --param 'caution' --param 'meal'
  HG20\x00\x0ccaution meal\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'caution' --param 'meal' | hg statbundle2
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

  $ hg bundle2 --param 'caution' --param 'meal=vegan' --param 'elephants' | hg statbundle2
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

  $ hg bundle2 --param 'e|! 7/=babar%#==tutu' --param simple | hg statbundle2
  options count: 2
  - e|! 7/
      babar%#==tutu
  - simple
  parts count:   0

Test unknown mandatory option
---------------------------------------------------

  $ hg bundle2 --param 'Gravity' | hg statbundle2
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

  $ hg statbundle2 --debug < ../out.hg2
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
  bundle part: "test:song"
  bundle part: "test:math"
  end of bundle

  $ cat ../parts.hg2
  HG20\x00\x00\x00\r (esc)
  test:empty\x00\x00\x00\x00\x00\x00\x00\r (esc)
  test:empty\x00\x00\x00\x00\x00\x00\x00\x0c	test:song\x00\x00\x00\x00\x00\xb2Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko (esc)
  Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.\x00\x00\x00\x00\x00'	test:math\x02\x01\x02\x04\x01\x04\x07\x03pi3.14e2.72cookingraw\x00\x00\x00\x0242\x00\x00\x00\x00\x00\x00 (no-eol) (esc)


  $ hg statbundle2 < ../parts.hg2
  options count: 0
  parts count:   4
    :test:empty:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
    :test:empty:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
    :test:song:
      mandatory: 0
      advisory: 0
      payload: 178 bytes
    :test:math:
      mandatory: 2
      advisory: 1
      payload: 2 bytes

  $ hg statbundle2 --debug < ../parts.hg2
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
  part header size: 12
  part type: "test:song"
  part parameters: 0
  payload chunk size: 178
  payload chunk size: 0
  part header size: 39
  part type: "test:math"
  part parameters: 3
  payload chunk size: 2
  payload chunk size: 0
  part header size: 0
  end of bundle2 stream
  parts count:   4
    :test:empty:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
    :test:empty:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
    :test:song:
      mandatory: 0
      advisory: 0
      payload: 178 bytes
    :test:math:
      mandatory: 2
      advisory: 1
      payload: 2 bytes

Test actual unbundling
========================

Process the bundle

  $ hg unbundle2 --debug < ../parts.hg2
  start processing of HG20 stream
  reading bundle2 stream parameters
  start extraction of bundle2 parts
  part header size: 13
  part type: "test:empty"
  part parameters: 0
  payload chunk size: 0
  ignoring unknown advisory part 'test:empty'
  part header size: 13
  part type: "test:empty"
  part parameters: 0
  payload chunk size: 0
  ignoring unknown advisory part 'test:empty'
  part header size: 12
  part type: "test:song"
  part parameters: 0
  payload chunk size: 178
  payload chunk size: 0
  found an handler for part 'test:song'
  The choir start singing:
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  part header size: 39
  part type: "test:math"
  part parameters: 3
  payload chunk size: 2
  payload chunk size: 0
  ignoring unknown advisory part 'test:math'
  part header size: 0
  end of bundle2 stream
  0 unread bytes


  $ hg bundle2 --parts --unknown ../unknown.hg2

  $ hg unbundle2 < ../unknown.hg2
  The choir start singing:
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  0 unread bytes
  abort: missing support for 'test:unknown'
  [255]
