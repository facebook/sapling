This test is dedicated to test the bundle2 container format

It test multiple existing parts to test different feature of the container. You
probably do not need to touch this test unless you change the binary encoding
of the bundle2 format itself.

Create an extension to test bundle2 API

  $ cat > bundle2.py << EOF
  > """A small extension to test bundle2 implementation
  > 
  > Current bundle2 implementation is far too limited to be used in any core
  > code. We still need to be able to test it while it grow up.
  > """
  > 
  > import sys, os
  > from mercurial import cmdutil
  > from mercurial import util
  > from mercurial import bundle2
  > from mercurial import scmutil
  > from mercurial import discovery
  > from mercurial import changegroup
  > from mercurial import error
  > from mercurial import obsolete
  > 
  > 
  > try:
  >     import msvcrt
  >     msvcrt.setmode(sys.stdin.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  >     msvcrt.setmode(sys.stderr.fileno(), os.O_BINARY)
  > except ImportError:
  >     pass
  > 
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > ELEPHANTSSONG = """Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
  > Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  > Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko."""
  > assert len(ELEPHANTSSONG) == 178 # future test say 178 bytes, trust it.
  > 
  > @bundle2.parthandler('test:song')
  > def songhandler(op, part):
  >     """handle a "test:song" bundle2 part, printing the lyrics on stdin"""
  >     op.ui.write('The choir starts singing:\n')
  >     verses = 0
  >     for line in part.read().split('\n'):
  >         op.ui.write('    %s\n' % line)
  >         verses += 1
  >     op.records.add('song', {'verses': verses})
  > 
  > @bundle2.parthandler('test:ping')
  > def pinghandler(op, part):
  >     op.ui.write('received ping request (id %i)\n' % part.id)
  >     if op.reply is not None and 'ping-pong' in op.reply.capabilities:
  >         op.ui.write_err('replying to ping request (id %i)\n' % part.id)
  >         op.reply.newpart('test:pong', [('in-reply-to', str(part.id))],
  >                          mandatory=False)
  > 
  > @bundle2.parthandler('test:debugreply')
  > def debugreply(op, part):
  >     """print data about the capacity of the bundle reply"""
  >     if op.reply is None:
  >         op.ui.write('debugreply: no reply\n')
  >     else:
  >         op.ui.write('debugreply: capabilities:\n')
  >         for cap in sorted(op.reply.capabilities):
  >             op.ui.write('debugreply:     %r\n' % cap)
  >             for val in op.reply.capabilities[cap]:
  >                 op.ui.write('debugreply:         %r\n' % val)
  > 
  > @command('bundle2',
  >          [('', 'param', [], 'stream level parameter'),
  >           ('', 'unknown', False, 'include an unknown mandatory part in the bundle'),
  >           ('', 'unknownparams', False, 'include an unknown part parameters in the bundle'),
  >           ('', 'parts', False, 'include some arbitrary parts to the bundle'),
  >           ('', 'reply', False, 'produce a reply bundle'),
  >           ('', 'pushrace', False, 'includes a check:head part with unknown nodes'),
  >           ('', 'genraise', False, 'includes a part that raise an exception during generation'),
  >           ('r', 'rev', [], 'includes those changeset in the bundle'),],
  >          '[OUTPUTFILE]')
  > def cmdbundle2(ui, repo, path=None, **opts):
  >     """write a bundle2 container on standard output"""
  >     bundler = bundle2.bundle20(ui)
  >     for p in opts['param']:
  >         p = p.split('=', 1)
  >         try:
  >             bundler.addparam(*p)
  >         except ValueError, exc:
  >             raise util.Abort('%s' % exc)
  > 
  >     if opts['reply']:
  >         capsstring = 'ping-pong\nelephants=babar,celeste\ncity%3D%21=celeste%2Cville'
  >         bundler.newpart('replycaps', data=capsstring)
  > 
  >     if opts['pushrace']:
  >         # also serve to test the assignement of data outside of init
  >         part = bundler.newpart('check:heads')
  >         part.data = '01234567890123456789'
  > 
  >     revs = opts['rev']
  >     if 'rev' in opts:
  >         revs = scmutil.revrange(repo, opts['rev'])
  >         if revs:
  >             # very crude version of a changegroup part creation
  >             bundled = repo.revs('%ld::%ld', revs, revs)
  >             headmissing = [c.node() for c in repo.set('heads(%ld)', revs)]
  >             headcommon  = [c.node() for c in repo.set('parents(%ld) - %ld', revs, revs)]
  >             outgoing = discovery.outgoing(repo.changelog, headcommon, headmissing)
  >             cg = changegroup.getlocalchangegroup(repo, 'test:bundle2', outgoing, None)
  >             bundler.newpart('changegroup', data=cg.getchunks(),
  >                             mandatory=False)
  > 
  >     if opts['parts']:
  >        bundler.newpart('test:empty', mandatory=False)
  >        # add a second one to make sure we handle multiple parts
  >        bundler.newpart('test:empty', mandatory=False)
  >        bundler.newpart('test:song', data=ELEPHANTSSONG, mandatory=False)
  >        bundler.newpart('test:debugreply', mandatory=False)
  >        mathpart = bundler.newpart('test:math')
  >        mathpart.addparam('pi', '3.14')
  >        mathpart.addparam('e', '2.72')
  >        mathpart.addparam('cooking', 'raw', mandatory=False)
  >        mathpart.data = '42'
  >        mathpart.mandatory = False
  >        # advisory known part with unknown mandatory param
  >        bundler.newpart('test:song', [('randomparam','')], mandatory=False)
  >     if opts['unknown']:
  >        bundler.newpart('test:unknown', data='some random content')
  >     if opts['unknownparams']:
  >        bundler.newpart('test:song', [('randomparams', '')])
  >     if opts['parts']:
  >        bundler.newpart('test:ping', mandatory=False)
  >     if opts['genraise']:
  >        def genraise():
  >            yield 'first line\n'
  >            raise RuntimeError('Someone set up us the bomb!')
  >        bundler.newpart('output', data=genraise(), mandatory=False)
  > 
  >     if path is None:
  >        file = sys.stdout
  >     else:
  >         file = open(path, 'wb')
  > 
  >     try:
  >         for chunk in bundler.getchunks():
  >             file.write(chunk)
  >     except RuntimeError, exc:
  >         raise util.Abort(exc)
  > 
  > @command('unbundle2', [], '')
  > def cmdunbundle2(ui, repo, replypath=None):
  >     """process a bundle2 stream from stdin on the current repo"""
  >     try:
  >         tr = None
  >         lock = repo.lock()
  >         tr = repo.transaction('processbundle')
  >         try:
  >             unbundler = bundle2.getunbundler(ui, sys.stdin)
  >             op = bundle2.processbundle(repo, unbundler, lambda: tr)
  >             tr.close()
  >         except error.BundleValueError, exc:
  >             raise util.Abort('missing support for %s' % exc)
  >         except error.PushRaced, exc:
  >             raise util.Abort('push race: %s' % exc)
  >     finally:
  >         if tr is not None:
  >             tr.release()
  >         lock.release()
  >         remains = sys.stdin.read()
  >         ui.write('%i unread bytes\n' % len(remains))
  >     if op.records['song']:
  >         totalverses = sum(r['verses'] for r in op.records['song'])
  >         ui.write('%i total verses sung\n' % totalverses)
  >     for rec in op.records['changegroup']:
  >         ui.write('addchangegroup return: %i\n' % rec['return'])
  >     if op.reply is not None and replypath is not None:
  >         file = open(replypath, 'wb')
  >         for chunk in op.reply.getchunks():
  >             file.write(chunk)
  > 
  > @command('statbundle2', [], '')
  > def cmdstatbundle2(ui, repo):
  >     """print statistic on the bundle2 container read from stdin"""
  >     unbundler = bundle2.getunbundler(ui, sys.stdin)
  >     try:
  >         params = unbundler.params
  >     except error.BundleValueError, exc:
  >        raise util.Abort('unknown parameters: %s' % exc)
  >     ui.write('options count: %i\n' % len(params))
  >     for key in sorted(params):
  >         ui.write('- %s\n' % key)
  >         value = params[key]
  >         if value is not None:
  >             ui.write('    %s\n' % value)
  >     count = 0
  >     for p in unbundler.iterparts():
  >         count += 1
  >         ui.write('  :%s:\n' % p.type)
  >         ui.write('    mandatory: %i\n' % len(p.mandatoryparams))
  >         ui.write('    advisory: %i\n' % len(p.advisoryparams))
  >         ui.write('    payload: %i bytes\n' % len(p.read()))
  >     ui.write('parts count:   %i\n' % count)
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > bundle2=$TESTTMP/bundle2.py
  > [experimental]
  > bundle2-exp=True
  > evolution=createmarkers
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > logtemplate={rev}:{node|short} {phase} {author} {bookmarks} {desc|firstline}
  > [web]
  > push_ssl = false
  > allow_push = *
  > [phases]
  > publish=False
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
  HG20\x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)

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
  HG20\x00\x00\x00\x07caution\x00\x00\x00\x00 (no-eol) (esc)

Test unbundling

  $ hg bundle2 --param 'caution' | hg statbundle2
  options count: 1
  - caution
  parts count:   0

Test generation multiple option

  $ hg bundle2 --param 'caution' --param 'meal'
  HG20\x00\x00\x00\x0ccaution meal\x00\x00\x00\x00 (no-eol) (esc)

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
  HG20\x00\x00\x00\x1ccaution meal=vegan elephants\x00\x00\x00\x00 (no-eol) (esc)

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
  HG20\x00\x00\x00)e%7C%21%207/=babar%25%23%3D%3Dtutu simple\x00\x00\x00\x00 (no-eol) (esc)

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
  abort: unknown parameters: Stream Parameter - Gravity
  [255]

Test debug output
---------------------------------------------------

bundling debug

  $ hg bundle2 --debug --param 'e|! 7/=babar%#==tutu' --param simple ../out.hg2 --config progress.debug=true
  bundle2-output-bundle: "HG20", (2 params) 0 parts total
  bundle2-output: start emission of HG20 stream
  bundle2-output: bundle parameter: e%7C%21%207/=babar%25%23%3D%3Dtutu simple
  bundle2-output: start of parts
  bundle2-output: end of bundle

file content is ok

  $ cat ../out.hg2
  HG20\x00\x00\x00)e%7C%21%207/=babar%25%23%3D%3Dtutu simple\x00\x00\x00\x00 (no-eol) (esc)

unbundling debug

  $ hg statbundle2 --debug --config progress.debug=true < ../out.hg2
  bundle2-input: start processing of HG20 stream
  bundle2-input: reading bundle2 stream parameters
  bundle2-input: ignoring unknown parameter 'e|! 7/'
  bundle2-input: ignoring unknown parameter 'simple'
  options count: 2
  - e|! 7/
      babar%#==tutu
  - simple
  bundle2-input: start extraction of bundle2 parts
  bundle2-input: part header size: 0
  bundle2-input: end of bundle2 stream
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

  $ hg bundle2 --parts ../parts.hg2 --debug --config progress.debug=true
  bundle2-output-bundle: "HG20", 7 parts total
  bundle2-output: start emission of HG20 stream
  bundle2-output: bundle parameter: 
  bundle2-output: start of parts
  bundle2-output: bundle part: "test:empty"
  bundle2-output-part: "test:empty" (advisory) empty payload
  bundle2-output: part 0: "test:empty"
  bundle2-output: header chunk size: 17
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:empty"
  bundle2-output-part: "test:empty" (advisory) empty payload
  bundle2-output: part 1: "test:empty"
  bundle2-output: header chunk size: 17
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:song"
  bundle2-output-part: "test:song" (advisory) 178 bytes payload
  bundle2-output: part 2: "test:song"
  bundle2-output: header chunk size: 16
  bundle2-output: payload chunk size: 178
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:debugreply"
  bundle2-output-part: "test:debugreply" (advisory) empty payload
  bundle2-output: part 3: "test:debugreply"
  bundle2-output: header chunk size: 22
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:math"
  bundle2-output-part: "test:math" (advisory) (params: 2 mandatory 2 advisory) 2 bytes payload
  bundle2-output: part 4: "test:math"
  bundle2-output: header chunk size: 43
  bundle2-output: payload chunk size: 2
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:song"
  bundle2-output-part: "test:song" (advisory) (params: 1 mandatory) empty payload
  bundle2-output: part 5: "test:song"
  bundle2-output: header chunk size: 29
  bundle2-output: closing payload chunk
  bundle2-output: bundle part: "test:ping"
  bundle2-output-part: "test:ping" (advisory) empty payload
  bundle2-output: part 6: "test:ping"
  bundle2-output: header chunk size: 16
  bundle2-output: closing payload chunk
  bundle2-output: end of bundle

  $ cat ../parts.hg2
  HG20\x00\x00\x00\x00\x00\x00\x00\x11 (esc)
  test:empty\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x11 (esc)
  test:empty\x00\x00\x00\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x10	test:song\x00\x00\x00\x02\x00\x00\x00\x00\x00\xb2Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko (esc)
  Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.\x00\x00\x00\x00\x00\x00\x00\x16\x0ftest:debugreply\x00\x00\x00\x03\x00\x00\x00\x00\x00\x00\x00\x00\x00+	test:math\x00\x00\x00\x04\x02\x01\x02\x04\x01\x04\x07\x03pi3.14e2.72cookingraw\x00\x00\x00\x0242\x00\x00\x00\x00\x00\x00\x00\x1d	test:song\x00\x00\x00\x05\x01\x00\x0b\x00randomparam\x00\x00\x00\x00\x00\x00\x00\x10	test:ping\x00\x00\x00\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)


  $ hg statbundle2 < ../parts.hg2
  options count: 0
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
    :test:debugreply:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
    :test:math:
      mandatory: 2
      advisory: 1
      payload: 2 bytes
    :test:song:
      mandatory: 1
      advisory: 0
      payload: 0 bytes
    :test:ping:
      mandatory: 0
      advisory: 0
      payload: 0 bytes
  parts count:   7

  $ hg statbundle2 --debug --config progress.debug=true < ../parts.hg2
  bundle2-input: start processing of HG20 stream
  bundle2-input: reading bundle2 stream parameters
  options count: 0
  bundle2-input: start extraction of bundle2 parts
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "0"
  bundle2-input: part parameters: 0
    :test:empty:
      mandatory: 0
      advisory: 0
  bundle2-input: payload chunk size: 0
      payload: 0 bytes
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "1"
  bundle2-input: part parameters: 0
    :test:empty:
      mandatory: 0
      advisory: 0
  bundle2-input: payload chunk size: 0
      payload: 0 bytes
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "2"
  bundle2-input: part parameters: 0
    :test:song:
      mandatory: 0
      advisory: 0
  bundle2-input: payload chunk size: 178
  bundle2-input: payload chunk size: 0
      payload: 178 bytes
  bundle2-input: part header size: 22
  bundle2-input: part type: "test:debugreply"
  bundle2-input: part id: "3"
  bundle2-input: part parameters: 0
    :test:debugreply:
      mandatory: 0
      advisory: 0
  bundle2-input: payload chunk size: 0
      payload: 0 bytes
  bundle2-input: part header size: 43
  bundle2-input: part type: "test:math"
  bundle2-input: part id: "4"
  bundle2-input: part parameters: 3
    :test:math:
      mandatory: 2
      advisory: 1
  bundle2-input: payload chunk size: 2
  bundle2-input: payload chunk size: 0
      payload: 2 bytes
  bundle2-input: part header size: 29
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "5"
  bundle2-input: part parameters: 1
    :test:song:
      mandatory: 1
      advisory: 0
  bundle2-input: payload chunk size: 0
      payload: 0 bytes
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:ping"
  bundle2-input: part id: "6"
  bundle2-input: part parameters: 0
    :test:ping:
      mandatory: 0
      advisory: 0
  bundle2-input: payload chunk size: 0
      payload: 0 bytes
  bundle2-input: part header size: 0
  bundle2-input: end of bundle2 stream
  parts count:   7

Test actual unbundling of test part
=======================================

Process the bundle

  $ hg unbundle2 --debug --config progress.debug=true < ../parts.hg2
  bundle2-input: start processing of HG20 stream
  bundle2-input: reading bundle2 stream parameters
  bundle2-input-bundle: with-transaction
  bundle2-input: start extraction of bundle2 parts
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "0"
  bundle2-input: part parameters: 0
  bundle2-input: ignoring unsupported advisory part test:empty
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "1"
  bundle2-input: part parameters: 0
  bundle2-input: ignoring unsupported advisory part test:empty
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "2"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:song'
  The choir starts singing:
  bundle2-input: payload chunk size: 178
  bundle2-input: payload chunk size: 0
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  bundle2-input: part header size: 22
  bundle2-input: part type: "test:debugreply"
  bundle2-input: part id: "3"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:debugreply'
  debugreply: no reply
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 43
  bundle2-input: part type: "test:math"
  bundle2-input: part id: "4"
  bundle2-input: part parameters: 3
  bundle2-input: ignoring unsupported advisory part test:math
  bundle2-input: payload chunk size: 2
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 29
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "5"
  bundle2-input: part parameters: 1
  bundle2-input: found a handler for part 'test:song'
  bundle2-input: ignoring unsupported advisory part test:song - randomparam
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:ping"
  bundle2-input: part id: "6"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:ping'
  received ping request (id 6)
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 0
  bundle2-input: end of bundle2 stream
  0 unread bytes
  3 total verses sung

Unbundle with an unknown mandatory part
(should abort)

  $ hg bundle2 --parts --unknown ../unknown.hg2

  $ hg unbundle2 < ../unknown.hg2
  The choir starts singing:
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  debugreply: no reply
  0 unread bytes
  abort: missing support for test:unknown
  [255]

Unbundle with an unknown mandatory part parameters
(should abort)

  $ hg bundle2 --unknownparams ../unknown.hg2

  $ hg unbundle2 < ../unknown.hg2
  0 unread bytes
  abort: missing support for test:song - randomparams
  [255]

unbundle with a reply

  $ hg bundle2 --parts --reply ../parts-reply.hg2
  $ hg unbundle2 ../reply.hg2 < ../parts-reply.hg2
  0 unread bytes
  3 total verses sung

The reply is a bundle

  $ cat ../reply.hg2
  HG20\x00\x00\x00\x00\x00\x00\x00\x1b\x06output\x00\x00\x00\x00\x00\x01\x0b\x01in-reply-to3\x00\x00\x00\xd9The choir starts singing: (esc)
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  \x00\x00\x00\x00\x00\x00\x00\x1b\x06output\x00\x00\x00\x01\x00\x01\x0b\x01in-reply-to4\x00\x00\x00\xc9debugreply: capabilities: (esc)
  debugreply:     'city=!'
  debugreply:         'celeste,ville'
  debugreply:     'elephants'
  debugreply:         'babar'
  debugreply:         'celeste'
  debugreply:     'ping-pong'
  \x00\x00\x00\x00\x00\x00\x00\x1e	test:pong\x00\x00\x00\x02\x01\x00\x0b\x01in-reply-to7\x00\x00\x00\x00\x00\x00\x00\x1b\x06output\x00\x00\x00\x03\x00\x01\x0b\x01in-reply-to7\x00\x00\x00=received ping request (id 7) (esc)
  replying to ping request (id 7)
  \x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)

The reply is valid

  $ hg statbundle2 < ../reply.hg2
  options count: 0
    :output:
      mandatory: 0
      advisory: 1
      payload: 217 bytes
    :output:
      mandatory: 0
      advisory: 1
      payload: 201 bytes
    :test:pong:
      mandatory: 1
      advisory: 0
      payload: 0 bytes
    :output:
      mandatory: 0
      advisory: 1
      payload: 61 bytes
  parts count:   4

Unbundle the reply to get the output:

  $ hg unbundle2 < ../reply.hg2
  remote: The choir starts singing:
  remote:     Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
  remote:     Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  remote:     Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  remote: debugreply: capabilities:
  remote: debugreply:     'city=!'
  remote: debugreply:         'celeste,ville'
  remote: debugreply:     'elephants'
  remote: debugreply:         'babar'
  remote: debugreply:         'celeste'
  remote: debugreply:     'ping-pong'
  remote: received ping request (id 7)
  remote: replying to ping request (id 7)
  0 unread bytes

Test push race detection

  $ hg bundle2 --pushrace ../part-race.hg2

  $ hg unbundle2 < ../part-race.hg2
  0 unread bytes
  abort: push race: repository changed while pushing - please try again
  [255]

Support for changegroup
===================================

  $ hg unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files (+3 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg log -G
  o  8:02de42196ebe draft Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  |
  | o  7:eea13746799a draft Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  |/|
  o |  6:24b6387c8c8c draft Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  | |
  | o  5:9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  |/
  | o  4:32af7686d403 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  | |
  | o  3:5fddd98957c8 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  | |
  | o  2:42ccdea3bb16 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  |/
  o  1:cd010b8cd998 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  @  0:3903775176ed draft test  a
  

  $ hg bundle2 --debug --config progress.debug=true --rev '8+7+5+4' ../rev.hg2
  4 changesets found
  list of changesets:
  32af7686d403cf45b5d95f2d70cebea587ac806a
  9520eea781bcca16c1e15acc0ba14335a0e8e5ba
  eea13746799a9e0bfd88f29d3c2e9dc9389f524f
  02de42196ebee42ef284b6780a87cdc96e8eaab6
  bundle2-output-bundle: "HG20", 1 parts total
  bundle2-output: start emission of HG20 stream
  bundle2-output: bundle parameter: 
  bundle2-output: start of parts
  bundle2-output: bundle part: "changegroup"
  bundle2-output-part: "changegroup" (advisory) streamed payload
  bundle2-output: part 0: "changegroup"
  bundle2-output: header chunk size: 18
  bundling: 1/4 changesets (25.00%)
  bundling: 2/4 changesets (50.00%)
  bundling: 3/4 changesets (75.00%)
  bundling: 4/4 changesets (100.00%)
  bundling: 1/4 manifests (25.00%)
  bundling: 2/4 manifests (50.00%)
  bundling: 3/4 manifests (75.00%)
  bundling: 4/4 manifests (100.00%)
  bundling: D 1/3 files (33.33%)
  bundling: E 2/3 files (66.67%)
  bundling: H 3/3 files (100.00%)
  bundle2-output: payload chunk size: 1555
  bundle2-output: closing payload chunk
  bundle2-output: end of bundle

  $ cat ../rev.hg2
  HG20\x00\x00\x00\x00\x00\x00\x00\x12\x0bchangegroup\x00\x00\x00\x00\x00\x00\x00\x00\x06\x13\x00\x00\x00\xa42\xafv\x86\xd4\x03\xcfE\xb5\xd9_-p\xce\xbe\xa5\x87\xac\x80j_\xdd\xd9\x89W\xc8\xa5JMCm\xfe\x1d\xa9\xd8\x7f!\xa1\xb9{\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x002\xafv\x86\xd4\x03\xcfE\xb5\xd9_-p\xce\xbe\xa5\x87\xac\x80j\x00\x00\x00\x00\x00\x00\x00)\x00\x00\x00)6e1f4c47ecb533ffd0c8e52cdc88afb6cd39e20c (esc)
  \x00\x00\x00f\x00\x00\x00h\x00\x00\x00\x02D (esc)
  \x00\x00\x00i\x00\x00\x00j\x00\x00\x00\x01D\x00\x00\x00\xa4\x95 \xee\xa7\x81\xbc\xca\x16\xc1\xe1Z\xcc\x0b\xa1C5\xa0\xe8\xe5\xba\xcd\x01\x0b\x8c\xd9\x98\xf3\x98\x1aZ\x81\x15\xf9O\x8d\xa4\xabP`\x89\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x95 \xee\xa7\x81\xbc\xca\x16\xc1\xe1Z\xcc\x0b\xa1C5\xa0\xe8\xe5\xba\x00\x00\x00\x00\x00\x00\x00)\x00\x00\x00)4dece9c826f69490507b98c6383a3009b295837d (esc)
  \x00\x00\x00f\x00\x00\x00h\x00\x00\x00\x02E (esc)
  \x00\x00\x00i\x00\x00\x00j\x00\x00\x00\x01E\x00\x00\x00\xa2\xee\xa17Fy\x9a\x9e\x0b\xfd\x88\xf2\x9d<.\x9d\xc98\x9fRO$\xb68|\x8c\x8c\xae7\x17\x88\x80\xf3\xfa\x95\xde\xd3\xcb\x1c\xf7\x85\x95 \xee\xa7\x81\xbc\xca\x16\xc1\xe1Z\xcc\x0b\xa1C5\xa0\xe8\xe5\xba\xee\xa17Fy\x9a\x9e\x0b\xfd\x88\xf2\x9d<.\x9d\xc98\x9fRO\x00\x00\x00\x00\x00\x00\x00)\x00\x00\x00)365b93d57fdf4814e2b5911d6bacff2b12014441 (esc)
  \x00\x00\x00f\x00\x00\x00h\x00\x00\x00\x00\x00\x00\x00i\x00\x00\x00j\x00\x00\x00\x01G\x00\x00\x00\xa4\x02\xdeB\x19n\xbe\xe4.\xf2\x84\xb6x (esc)
  \x87\xcd\xc9n\x8e\xaa\xb6$\xb68|\x8c\x8c\xae7\x17\x88\x80\xf3\xfa\x95\xde\xd3\xcb\x1c\xf7\x85\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\xdeB\x19n\xbe\xe4.\xf2\x84\xb6x (esc)
  \x87\xcd\xc9n\x8e\xaa\xb6\x00\x00\x00\x00\x00\x00\x00)\x00\x00\x00)8bee48edc7318541fc0013ee41b089276a8c24bf (esc)
  \x00\x00\x00f\x00\x00\x00f\x00\x00\x00\x02H (esc)
  \x00\x00\x00g\x00\x00\x00h\x00\x00\x00\x01H\x00\x00\x00\x00\x00\x00\x00\x8bn\x1fLG\xec\xb53\xff\xd0\xc8\xe5,\xdc\x88\xaf\xb6\xcd9\xe2\x0cf\xa5\xa0\x18\x17\xfd\xf5#\x9c'8\x02\xb5\xb7a\x8d\x05\x1c\x89\xe4\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x002\xafv\x86\xd4\x03\xcfE\xb5\xd9_-p\xce\xbe\xa5\x87\xac\x80j\x00\x00\x00\x81\x00\x00\x00\x81\x00\x00\x00+D\x00c3f1ca2924c16a19b0656a84900e504e5b0aec2d (esc)
  \x00\x00\x00\x8bM\xec\xe9\xc8&\xf6\x94\x90P{\x98\xc68:0	\xb2\x95\x83}\x00}\x8c\x9d\x88\x84\x13%\xf5\xc6\xb0cq\xb3[N\x8a+\x1a\x83\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x95 \xee\xa7\x81\xbc\xca\x16\xc1\xe1Z\xcc\x0b\xa1C5\xa0\xe8\xe5\xba\x00\x00\x00+\x00\x00\x00\xac\x00\x00\x00+E\x009c6fd0350a6c0d0c49d4a9c5017cf07043f54e58 (esc)
  \x00\x00\x00\x8b6[\x93\xd5\x7f\xdfH\x14\xe2\xb5\x91\x1dk\xac\xff+\x12\x01DA(\xa5\x84\xc6^\xf1!\xf8\x9e\xb6j\xb7\xd0\xbc\x15=\x80\x99\xe7\xceM\xec\xe9\xc8&\xf6\x94\x90P{\x98\xc68:0	\xb2\x95\x83}\xee\xa17Fy\x9a\x9e\x0b\xfd\x88\xf2\x9d<.\x9d\xc98\x9fRO\x00\x00\x00V\x00\x00\x00V\x00\x00\x00+F\x0022bfcfd62a21a3287edbd4d656218d0f525ed76a (esc)
  \x00\x00\x00\x97\x8b\xeeH\xed\xc71\x85A\xfc\x00\x13\xeeA\xb0\x89'j\x8c$\xbf(\xa5\x84\xc6^\xf1!\xf8\x9e\xb6j\xb7\xd0\xbc\x15=\x80\x99\xe7\xce\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\xdeB\x19n\xbe\xe4.\xf2\x84\xb6x (esc)
  \x87\xcd\xc9n\x8e\xaa\xb6\x00\x00\x00+\x00\x00\x00V\x00\x00\x00\x00\x00\x00\x00\x81\x00\x00\x00\x81\x00\x00\x00+H\x008500189e74a9e0475e822093bc7db0d631aeb0b4 (esc)
  \x00\x00\x00\x00\x00\x00\x00\x05D\x00\x00\x00b\xc3\xf1\xca)$\xc1j\x19\xb0ej\x84\x90\x0ePN[ (esc)
  \xec-\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x002\xafv\x86\xd4\x03\xcfE\xb5\xd9_-p\xce\xbe\xa5\x87\xac\x80j\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02D (esc)
  \x00\x00\x00\x00\x00\x00\x00\x05E\x00\x00\x00b\x9co\xd05 (esc)
  l\r (no-eol) (esc)
  \x0cI\xd4\xa9\xc5\x01|\xf0pC\xf5NX\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x95 \xee\xa7\x81\xbc\xca\x16\xc1\xe1Z\xcc\x0b\xa1C5\xa0\xe8\xe5\xba\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02E (esc)
  \x00\x00\x00\x00\x00\x00\x00\x05H\x00\x00\x00b\x85\x00\x18\x9et\xa9\xe0G^\x82 \x93\xbc}\xb0\xd61\xae\xb0\xb4\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02\xdeB\x19n\xbe\xe4.\xf2\x84\xb6x (esc)
  \x87\xcd\xc9n\x8e\xaa\xb6\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x02H (esc)
  \x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)

  $ hg debugbundle ../rev.hg2
  Stream params: {}
  changegroup -- '{}'
      32af7686d403cf45b5d95f2d70cebea587ac806a
      9520eea781bcca16c1e15acc0ba14335a0e8e5ba
      eea13746799a9e0bfd88f29d3c2e9dc9389f524f
      02de42196ebee42ef284b6780a87cdc96e8eaab6
  $ hg unbundle ../rev.hg2
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 3 files

with reply

  $ hg bundle2 --rev '8+7+5+4' --reply ../rev-rr.hg2
  $ hg unbundle2 ../rev-reply.hg2 < ../rev-rr.hg2
  0 unread bytes
  addchangegroup return: 1

  $ cat ../rev-reply.hg2
  HG20\x00\x00\x00\x00\x00\x00\x00/\x11reply:changegroup\x00\x00\x00\x00\x00\x02\x0b\x01\x06\x01in-reply-to1return1\x00\x00\x00\x00\x00\x00\x00\x1b\x06output\x00\x00\x00\x01\x00\x01\x0b\x01in-reply-to1\x00\x00\x00dadding changesets (esc)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 3 files
  \x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)

Check handling of exception during generation.
----------------------------------------------

  $ hg bundle2 --genraise > ../genfailed.hg2
  abort: Someone set up us the bomb!
  [255]

Should still be a valid bundle

  $ cat ../genfailed.hg2
  HG20\x00\x00\x00\x00\x00\x00\x00\r (no-eol) (esc)
  \x06output\x00\x00\x00\x00\x00\x00\xff\xff\xff\xff\x00\x00\x00H\x0berror:abort\x00\x00\x00\x00\x01\x00\x07-messageunexpected error: Someone set up us the bomb!\x00\x00\x00\x00\x00\x00\x00\x00 (no-eol) (esc)

And its handling on the other size raise a clean exception

  $ cat ../genfailed.hg2 | hg unbundle2
  0 unread bytes
  abort: unexpected error: Someone set up us the bomb!
  [255]


  $ cd ..
