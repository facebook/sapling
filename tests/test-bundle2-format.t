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
  >           ('', 'timeout', False, 'emulate a timeout during bundle generation'),
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
  >     if opts['timeout']:
  >         bundler.newpart('test:song', data=ELEPHANTSSONG, mandatory=False)
  >         for idx, junk in enumerate(bundler.getchunks()):
  >             ui.write('%d chunk\n' % idx)
  >             if idx > 4:
  >                 # This throws a GeneratorExit inside the generator, which
  >                 # can cause problems if the exception-recovery code is
  >                 # too zealous. It's important for this test that the break
  >                 # occur while we're in the middle of a part.
  >                 break
  >         ui.write('fake timeout complete.\n')
  >         return
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

  $ hg bundle2 | f --hexdump
  
  0000: 48 47 32 30 00 00 00 00 00 00 00 00             |HG20........|

Test timeouts during bundling
  $ hg bundle2 --timeout --debug --config devel.bundle2.debug=yes
  bundle2-output-bundle: "HG20", 1 parts total
  bundle2-output: start emission of HG20 stream
  0 chunk
  bundle2-output: bundle parameter: 
  1 chunk
  bundle2-output: start of parts
  bundle2-output: bundle part: "test:song"
  bundle2-output-part: "test:song" (advisory) 178 bytes payload
  bundle2-output: part 0: "test:song"
  bundle2-output: header chunk size: 16
  2 chunk
  3 chunk
  bundle2-output: payload chunk size: 178
  4 chunk
  5 chunk
  bundle2-generatorexit
  fake timeout complete.

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

  $ hg bundle2 --param 'caution' | f --hexdump
  
  0000: 48 47 32 30 00 00 00 07 63 61 75 74 69 6f 6e 00 |HG20....caution.|
  0010: 00 00 00                                        |...|

Test unbundling

  $ hg bundle2 --param 'caution' | hg statbundle2
  options count: 1
  - caution
  parts count:   0

Test generation multiple option

  $ hg bundle2 --param 'caution' --param 'meal' | f --hexdump
  
  0000: 48 47 32 30 00 00 00 0c 63 61 75 74 69 6f 6e 20 |HG20....caution |
  0010: 6d 65 61 6c 00 00 00 00                         |meal....|

Test unbundling

  $ hg bundle2 --param 'caution' --param 'meal' | hg statbundle2
  options count: 2
  - caution
  - meal
  parts count:   0

advisory parameters, with value
-------------------------------

Test generation

  $ hg bundle2 --param 'caution' --param 'meal=vegan' --param 'elephants' | f --hexdump
  
  0000: 48 47 32 30 00 00 00 1c 63 61 75 74 69 6f 6e 20 |HG20....caution |
  0010: 6d 65 61 6c 3d 76 65 67 61 6e 20 65 6c 65 70 68 |meal=vegan eleph|
  0020: 61 6e 74 73 00 00 00 00                         |ants....|

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

  $ hg bundle2 --param 'e|! 7/=babar%#==tutu' --param simple | f --hexdump
  
  0000: 48 47 32 30 00 00 00 29 65 25 37 43 25 32 31 25 |HG20...)e%7C%21%|
  0010: 32 30 37 2f 3d 62 61 62 61 72 25 32 35 25 32 33 |207/=babar%25%23|
  0020: 25 33 44 25 33 44 74 75 74 75 20 73 69 6d 70 6c |%3D%3Dtutu simpl|
  0030: 65 00 00 00 00                                  |e....|

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

  $ hg bundle2 --debug --param 'e|! 7/=babar%#==tutu' --param simple ../out.hg2 --config progress.debug=true --config devel.bundle2.debug=true
  bundle2-output-bundle: "HG20", (2 params) 0 parts total
  bundle2-output: start emission of HG20 stream
  bundle2-output: bundle parameter: e%7C%21%207/=babar%25%23%3D%3Dtutu simple
  bundle2-output: start of parts
  bundle2-output: end of bundle

file content is ok

  $ f --hexdump ../out.hg2
  ../out.hg2:
  0000: 48 47 32 30 00 00 00 29 65 25 37 43 25 32 31 25 |HG20...)e%7C%21%|
  0010: 32 30 37 2f 3d 62 61 62 61 72 25 32 35 25 32 33 |207/=babar%25%23|
  0020: 25 33 44 25 33 44 74 75 74 75 20 73 69 6d 70 6c |%3D%3Dtutu simpl|
  0030: 65 00 00 00 00                                  |e....|

unbundling debug

  $ hg statbundle2 --debug --config progress.debug=true --config devel.bundle2.debug=true < ../out.hg2
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

  $ hg bundle2 --parts ../parts.hg2 --debug --config progress.debug=true --config devel.bundle2.debug=true
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

  $ f --hexdump ../parts.hg2
  ../parts.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 11 0a 74 65 73 |HG20.........tes|
  0010: 74 3a 65 6d 70 74 79 00 00 00 00 00 00 00 00 00 |t:empty.........|
  0020: 00 00 00 00 11 0a 74 65 73 74 3a 65 6d 70 74 79 |......test:empty|
  0030: 00 00 00 01 00 00 00 00 00 00 00 00 00 10 09 74 |...............t|
  0040: 65 73 74 3a 73 6f 6e 67 00 00 00 02 00 00 00 00 |est:song........|
  0050: 00 b2 50 61 74 61 6c 69 20 44 69 72 61 70 61 74 |..Patali Dirapat|
  0060: 61 2c 20 43 72 6f 6d 64 61 20 43 72 6f 6d 64 61 |a, Cromda Cromda|
  0070: 20 52 69 70 61 6c 6f 2c 20 50 61 74 61 20 50 61 | Ripalo, Pata Pa|
  0080: 74 61 2c 20 4b 6f 20 4b 6f 20 4b 6f 0a 42 6f 6b |ta, Ko Ko Ko.Bok|
  0090: 6f 72 6f 20 44 69 70 6f 75 6c 69 74 6f 2c 20 52 |oro Dipoulito, R|
  00a0: 6f 6e 64 69 20 52 6f 6e 64 69 20 50 65 70 69 6e |ondi Rondi Pepin|
  00b0: 6f 2c 20 50 61 74 61 20 50 61 74 61 2c 20 4b 6f |o, Pata Pata, Ko|
  00c0: 20 4b 6f 20 4b 6f 0a 45 6d 61 6e 61 20 4b 61 72 | Ko Ko.Emana Kar|
  00d0: 61 73 73 6f 6c 69 2c 20 4c 6f 75 63 72 61 20 4c |assoli, Loucra L|
  00e0: 6f 75 63 72 61 20 50 6f 6e 70 6f 6e 74 6f 2c 20 |oucra Ponponto, |
  00f0: 50 61 74 61 20 50 61 74 61 2c 20 4b 6f 20 4b 6f |Pata Pata, Ko Ko|
  0100: 20 4b 6f 2e 00 00 00 00 00 00 00 16 0f 74 65 73 | Ko..........tes|
  0110: 74 3a 64 65 62 75 67 72 65 70 6c 79 00 00 00 03 |t:debugreply....|
  0120: 00 00 00 00 00 00 00 00 00 2b 09 74 65 73 74 3a |.........+.test:|
  0130: 6d 61 74 68 00 00 00 04 02 01 02 04 01 04 07 03 |math............|
  0140: 70 69 33 2e 31 34 65 32 2e 37 32 63 6f 6f 6b 69 |pi3.14e2.72cooki|
  0150: 6e 67 72 61 77 00 00 00 02 34 32 00 00 00 00 00 |ngraw....42.....|
  0160: 00 00 1d 09 74 65 73 74 3a 73 6f 6e 67 00 00 00 |....test:song...|
  0170: 05 01 00 0b 00 72 61 6e 64 6f 6d 70 61 72 61 6d |.....randomparam|
  0180: 00 00 00 00 00 00 00 10 09 74 65 73 74 3a 70 69 |.........test:pi|
  0190: 6e 67 00 00 00 06 00 00 00 00 00 00 00 00 00 00 |ng..............|


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

  $ hg statbundle2 --debug --config progress.debug=true --config devel.bundle2.debug=true < ../parts.hg2
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
  bundle2-input-part: total payload size 178
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
  bundle2-input-part: total payload size 2
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

  $ hg unbundle2 --debug --config progress.debug=true --config devel.bundle2.debug=true < ../parts.hg2
  bundle2-input: start processing of HG20 stream
  bundle2-input: reading bundle2 stream parameters
  bundle2-input-bundle: with-transaction
  bundle2-input: start extraction of bundle2 parts
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "0"
  bundle2-input: part parameters: 0
  bundle2-input: ignoring unsupported advisory part test:empty
  bundle2-input-part: "test:empty" (advisory) unsupported-type
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 17
  bundle2-input: part type: "test:empty"
  bundle2-input: part id: "1"
  bundle2-input: part parameters: 0
  bundle2-input: ignoring unsupported advisory part test:empty
  bundle2-input-part: "test:empty" (advisory) unsupported-type
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "2"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:song'
  bundle2-input-part: "test:song" (advisory) supported
  The choir starts singing:
  bundle2-input: payload chunk size: 178
  bundle2-input: payload chunk size: 0
  bundle2-input-part: total payload size 178
      Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
      Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
      Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko.
  bundle2-input: part header size: 22
  bundle2-input: part type: "test:debugreply"
  bundle2-input: part id: "3"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:debugreply'
  bundle2-input-part: "test:debugreply" (advisory) supported
  debugreply: no reply
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 43
  bundle2-input: part type: "test:math"
  bundle2-input: part id: "4"
  bundle2-input: part parameters: 3
  bundle2-input: ignoring unsupported advisory part test:math
  bundle2-input-part: "test:math" (advisory) (params: 2 mandatory 2 advisory) unsupported-type
  bundle2-input: payload chunk size: 2
  bundle2-input: payload chunk size: 0
  bundle2-input-part: total payload size 2
  bundle2-input: part header size: 29
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "5"
  bundle2-input: part parameters: 1
  bundle2-input: found a handler for part 'test:song'
  bundle2-input: ignoring unsupported advisory part test:song - randomparam
  bundle2-input-part: "test:song" (advisory) (params: 1 mandatory) unsupported-params (['randomparam'])
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:ping"
  bundle2-input: part id: "6"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part 'test:ping'
  bundle2-input-part: "test:ping" (advisory) supported
  received ping request (id 6)
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 0
  bundle2-input: end of bundle2 stream
  bundle2-input-bundle: 6 parts total
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

  $ f --hexdump ../reply.hg2
  ../reply.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 1b 06 6f 75 74 |HG20.........out|
  0010: 70 75 74 00 00 00 00 00 01 0b 01 69 6e 2d 72 65 |put........in-re|
  0020: 70 6c 79 2d 74 6f 33 00 00 00 d9 54 68 65 20 63 |ply-to3....The c|
  0030: 68 6f 69 72 20 73 74 61 72 74 73 20 73 69 6e 67 |hoir starts sing|
  0040: 69 6e 67 3a 0a 20 20 20 20 50 61 74 61 6c 69 20 |ing:.    Patali |
  0050: 44 69 72 61 70 61 74 61 2c 20 43 72 6f 6d 64 61 |Dirapata, Cromda|
  0060: 20 43 72 6f 6d 64 61 20 52 69 70 61 6c 6f 2c 20 | Cromda Ripalo, |
  0070: 50 61 74 61 20 50 61 74 61 2c 20 4b 6f 20 4b 6f |Pata Pata, Ko Ko|
  0080: 20 4b 6f 0a 20 20 20 20 42 6f 6b 6f 72 6f 20 44 | Ko.    Bokoro D|
  0090: 69 70 6f 75 6c 69 74 6f 2c 20 52 6f 6e 64 69 20 |ipoulito, Rondi |
  00a0: 52 6f 6e 64 69 20 50 65 70 69 6e 6f 2c 20 50 61 |Rondi Pepino, Pa|
  00b0: 74 61 20 50 61 74 61 2c 20 4b 6f 20 4b 6f 20 4b |ta Pata, Ko Ko K|
  00c0: 6f 0a 20 20 20 20 45 6d 61 6e 61 20 4b 61 72 61 |o.    Emana Kara|
  00d0: 73 73 6f 6c 69 2c 20 4c 6f 75 63 72 61 20 4c 6f |ssoli, Loucra Lo|
  00e0: 75 63 72 61 20 50 6f 6e 70 6f 6e 74 6f 2c 20 50 |ucra Ponponto, P|
  00f0: 61 74 61 20 50 61 74 61 2c 20 4b 6f 20 4b 6f 20 |ata Pata, Ko Ko |
  0100: 4b 6f 2e 0a 00 00 00 00 00 00 00 1b 06 6f 75 74 |Ko...........out|
  0110: 70 75 74 00 00 00 01 00 01 0b 01 69 6e 2d 72 65 |put........in-re|
  0120: 70 6c 79 2d 74 6f 34 00 00 00 c9 64 65 62 75 67 |ply-to4....debug|
  0130: 72 65 70 6c 79 3a 20 63 61 70 61 62 69 6c 69 74 |reply: capabilit|
  0140: 69 65 73 3a 0a 64 65 62 75 67 72 65 70 6c 79 3a |ies:.debugreply:|
  0150: 20 20 20 20 20 27 63 69 74 79 3d 21 27 0a 64 65 |     'city=!'.de|
  0160: 62 75 67 72 65 70 6c 79 3a 20 20 20 20 20 20 20 |bugreply:       |
  0170: 20 20 27 63 65 6c 65 73 74 65 2c 76 69 6c 6c 65 |  'celeste,ville|
  0180: 27 0a 64 65 62 75 67 72 65 70 6c 79 3a 20 20 20 |'.debugreply:   |
  0190: 20 20 27 65 6c 65 70 68 61 6e 74 73 27 0a 64 65 |  'elephants'.de|
  01a0: 62 75 67 72 65 70 6c 79 3a 20 20 20 20 20 20 20 |bugreply:       |
  01b0: 20 20 27 62 61 62 61 72 27 0a 64 65 62 75 67 72 |  'babar'.debugr|
  01c0: 65 70 6c 79 3a 20 20 20 20 20 20 20 20 20 27 63 |eply:         'c|
  01d0: 65 6c 65 73 74 65 27 0a 64 65 62 75 67 72 65 70 |eleste'.debugrep|
  01e0: 6c 79 3a 20 20 20 20 20 27 70 69 6e 67 2d 70 6f |ly:     'ping-po|
  01f0: 6e 67 27 0a 00 00 00 00 00 00 00 1e 09 74 65 73 |ng'..........tes|
  0200: 74 3a 70 6f 6e 67 00 00 00 02 01 00 0b 01 69 6e |t:pong........in|
  0210: 2d 72 65 70 6c 79 2d 74 6f 37 00 00 00 00 00 00 |-reply-to7......|
  0220: 00 1b 06 6f 75 74 70 75 74 00 00 00 03 00 01 0b |...output.......|
  0230: 01 69 6e 2d 72 65 70 6c 79 2d 74 6f 37 00 00 00 |.in-reply-to7...|
  0240: 3d 72 65 63 65 69 76 65 64 20 70 69 6e 67 20 72 |=received ping r|
  0250: 65 71 75 65 73 74 20 28 69 64 20 37 29 0a 72 65 |equest (id 7).re|
  0260: 70 6c 79 69 6e 67 20 74 6f 20 70 69 6e 67 20 72 |plying to ping r|
  0270: 65 71 75 65 73 74 20 28 69 64 20 37 29 0a 00 00 |equest (id 7)...|
  0280: 00 00 00 00 00 00                               |......|

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
  

  $ hg bundle2 --debug --config progress.debug=true --config devel.bundle2.debug=true --rev '8+7+5+4' ../rev.hg2
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

  $ f --hexdump ../rev.hg2
  ../rev.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 12 0b 63 68 61 |HG20.........cha|
  0010: 6e 67 65 67 72 6f 75 70 00 00 00 00 00 00 00 00 |ngegroup........|
  0020: 06 13 00 00 00 a4 32 af 76 86 d4 03 cf 45 b5 d9 |......2.v....E..|
  0030: 5f 2d 70 ce be a5 87 ac 80 6a 5f dd d9 89 57 c8 |_-p......j_...W.|
  0040: a5 4a 4d 43 6d fe 1d a9 d8 7f 21 a1 b9 7b 00 00 |.JMCm.....!..{..|
  0050: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0060: 00 00 32 af 76 86 d4 03 cf 45 b5 d9 5f 2d 70 ce |..2.v....E.._-p.|
  0070: be a5 87 ac 80 6a 00 00 00 00 00 00 00 29 00 00 |.....j.......)..|
  0080: 00 29 36 65 31 66 34 63 34 37 65 63 62 35 33 33 |.)6e1f4c47ecb533|
  0090: 66 66 64 30 63 38 65 35 32 63 64 63 38 38 61 66 |ffd0c8e52cdc88af|
  00a0: 62 36 63 64 33 39 65 32 30 63 0a 00 00 00 66 00 |b6cd39e20c....f.|
  00b0: 00 00 68 00 00 00 02 44 0a 00 00 00 69 00 00 00 |..h....D....i...|
  00c0: 6a 00 00 00 01 44 00 00 00 a4 95 20 ee a7 81 bc |j....D..... ....|
  00d0: ca 16 c1 e1 5a cc 0b a1 43 35 a0 e8 e5 ba cd 01 |....Z...C5......|
  00e0: 0b 8c d9 98 f3 98 1a 5a 81 15 f9 4f 8d a4 ab 50 |.......Z...O...P|
  00f0: 60 89 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |`...............|
  0100: 00 00 00 00 00 00 95 20 ee a7 81 bc ca 16 c1 e1 |....... ........|
  0110: 5a cc 0b a1 43 35 a0 e8 e5 ba 00 00 00 00 00 00 |Z...C5..........|
  0120: 00 29 00 00 00 29 34 64 65 63 65 39 63 38 32 36 |.)...)4dece9c826|
  0130: 66 36 39 34 39 30 35 30 37 62 39 38 63 36 33 38 |f69490507b98c638|
  0140: 33 61 33 30 30 39 62 32 39 35 38 33 37 64 0a 00 |3a3009b295837d..|
  0150: 00 00 66 00 00 00 68 00 00 00 02 45 0a 00 00 00 |..f...h....E....|
  0160: 69 00 00 00 6a 00 00 00 01 45 00 00 00 a2 ee a1 |i...j....E......|
  0170: 37 46 79 9a 9e 0b fd 88 f2 9d 3c 2e 9d c9 38 9f |7Fy.......<...8.|
  0180: 52 4f 24 b6 38 7c 8c 8c ae 37 17 88 80 f3 fa 95 |RO$.8|...7......|
  0190: de d3 cb 1c f7 85 95 20 ee a7 81 bc ca 16 c1 e1 |....... ........|
  01a0: 5a cc 0b a1 43 35 a0 e8 e5 ba ee a1 37 46 79 9a |Z...C5......7Fy.|
  01b0: 9e 0b fd 88 f2 9d 3c 2e 9d c9 38 9f 52 4f 00 00 |......<...8.RO..|
  01c0: 00 00 00 00 00 29 00 00 00 29 33 36 35 62 39 33 |.....)...)365b93|
  01d0: 64 35 37 66 64 66 34 38 31 34 65 32 62 35 39 31 |d57fdf4814e2b591|
  01e0: 31 64 36 62 61 63 66 66 32 62 31 32 30 31 34 34 |1d6bacff2b120144|
  01f0: 34 31 0a 00 00 00 66 00 00 00 68 00 00 00 00 00 |41....f...h.....|
  0200: 00 00 69 00 00 00 6a 00 00 00 01 47 00 00 00 a4 |..i...j....G....|
  0210: 02 de 42 19 6e be e4 2e f2 84 b6 78 0a 87 cd c9 |..B.n......x....|
  0220: 6e 8e aa b6 24 b6 38 7c 8c 8c ae 37 17 88 80 f3 |n...$.8|...7....|
  0230: fa 95 de d3 cb 1c f7 85 00 00 00 00 00 00 00 00 |................|
  0240: 00 00 00 00 00 00 00 00 00 00 00 00 02 de 42 19 |..............B.|
  0250: 6e be e4 2e f2 84 b6 78 0a 87 cd c9 6e 8e aa b6 |n......x....n...|
  0260: 00 00 00 00 00 00 00 29 00 00 00 29 38 62 65 65 |.......)...)8bee|
  0270: 34 38 65 64 63 37 33 31 38 35 34 31 66 63 30 30 |48edc7318541fc00|
  0280: 31 33 65 65 34 31 62 30 38 39 32 37 36 61 38 63 |13ee41b089276a8c|
  0290: 32 34 62 66 0a 00 00 00 66 00 00 00 66 00 00 00 |24bf....f...f...|
  02a0: 02 48 0a 00 00 00 67 00 00 00 68 00 00 00 01 48 |.H....g...h....H|
  02b0: 00 00 00 00 00 00 00 8b 6e 1f 4c 47 ec b5 33 ff |........n.LG..3.|
  02c0: d0 c8 e5 2c dc 88 af b6 cd 39 e2 0c 66 a5 a0 18 |...,.....9..f...|
  02d0: 17 fd f5 23 9c 27 38 02 b5 b7 61 8d 05 1c 89 e4 |...#.'8...a.....|
  02e0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  02f0: 00 00 00 00 32 af 76 86 d4 03 cf 45 b5 d9 5f 2d |....2.v....E.._-|
  0300: 70 ce be a5 87 ac 80 6a 00 00 00 81 00 00 00 81 |p......j........|
  0310: 00 00 00 2b 44 00 63 33 66 31 63 61 32 39 32 34 |...+D.c3f1ca2924|
  0320: 63 31 36 61 31 39 62 30 36 35 36 61 38 34 39 30 |c16a19b0656a8490|
  0330: 30 65 35 30 34 65 35 62 30 61 65 63 32 64 0a 00 |0e504e5b0aec2d..|
  0340: 00 00 8b 4d ec e9 c8 26 f6 94 90 50 7b 98 c6 38 |...M...&...P{..8|
  0350: 3a 30 09 b2 95 83 7d 00 7d 8c 9d 88 84 13 25 f5 |:0....}.}.....%.|
  0360: c6 b0 63 71 b3 5b 4e 8a 2b 1a 83 00 00 00 00 00 |..cq.[N.+.......|
  0370: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 95 |................|
  0380: 20 ee a7 81 bc ca 16 c1 e1 5a cc 0b a1 43 35 a0 | ........Z...C5.|
  0390: e8 e5 ba 00 00 00 2b 00 00 00 ac 00 00 00 2b 45 |......+.......+E|
  03a0: 00 39 63 36 66 64 30 33 35 30 61 36 63 30 64 30 |.9c6fd0350a6c0d0|
  03b0: 63 34 39 64 34 61 39 63 35 30 31 37 63 66 30 37 |c49d4a9c5017cf07|
  03c0: 30 34 33 66 35 34 65 35 38 0a 00 00 00 8b 36 5b |043f54e58.....6[|
  03d0: 93 d5 7f df 48 14 e2 b5 91 1d 6b ac ff 2b 12 01 |....H.....k..+..|
  03e0: 44 41 28 a5 84 c6 5e f1 21 f8 9e b6 6a b7 d0 bc |DA(...^.!...j...|
  03f0: 15 3d 80 99 e7 ce 4d ec e9 c8 26 f6 94 90 50 7b |.=....M...&...P{|
  0400: 98 c6 38 3a 30 09 b2 95 83 7d ee a1 37 46 79 9a |..8:0....}..7Fy.|
  0410: 9e 0b fd 88 f2 9d 3c 2e 9d c9 38 9f 52 4f 00 00 |......<...8.RO..|
  0420: 00 56 00 00 00 56 00 00 00 2b 46 00 32 32 62 66 |.V...V...+F.22bf|
  0430: 63 66 64 36 32 61 32 31 61 33 32 38 37 65 64 62 |cfd62a21a3287edb|
  0440: 64 34 64 36 35 36 32 31 38 64 30 66 35 32 35 65 |d4d656218d0f525e|
  0450: 64 37 36 61 0a 00 00 00 97 8b ee 48 ed c7 31 85 |d76a.......H..1.|
  0460: 41 fc 00 13 ee 41 b0 89 27 6a 8c 24 bf 28 a5 84 |A....A..'j.$.(..|
  0470: c6 5e f1 21 f8 9e b6 6a b7 d0 bc 15 3d 80 99 e7 |.^.!...j....=...|
  0480: ce 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0490: 00 00 00 00 00 02 de 42 19 6e be e4 2e f2 84 b6 |.......B.n......|
  04a0: 78 0a 87 cd c9 6e 8e aa b6 00 00 00 2b 00 00 00 |x....n......+...|
  04b0: 56 00 00 00 00 00 00 00 81 00 00 00 81 00 00 00 |V...............|
  04c0: 2b 48 00 38 35 30 30 31 38 39 65 37 34 61 39 65 |+H.8500189e74a9e|
  04d0: 30 34 37 35 65 38 32 32 30 39 33 62 63 37 64 62 |0475e822093bc7db|
  04e0: 30 64 36 33 31 61 65 62 30 62 34 0a 00 00 00 00 |0d631aeb0b4.....|
  04f0: 00 00 00 05 44 00 00 00 62 c3 f1 ca 29 24 c1 6a |....D...b...)$.j|
  0500: 19 b0 65 6a 84 90 0e 50 4e 5b 0a ec 2d 00 00 00 |..ej...PN[..-...|
  0510: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0520: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0530: 00 00 00 00 00 32 af 76 86 d4 03 cf 45 b5 d9 5f |.....2.v....E.._|
  0540: 2d 70 ce be a5 87 ac 80 6a 00 00 00 00 00 00 00 |-p......j.......|
  0550: 00 00 00 00 02 44 0a 00 00 00 00 00 00 00 05 45 |.....D.........E|
  0560: 00 00 00 62 9c 6f d0 35 0a 6c 0d 0c 49 d4 a9 c5 |...b.o.5.l..I...|
  0570: 01 7c f0 70 43 f5 4e 58 00 00 00 00 00 00 00 00 |.|.pC.NX........|
  0580: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0590: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  05a0: 95 20 ee a7 81 bc ca 16 c1 e1 5a cc 0b a1 43 35 |. ........Z...C5|
  05b0: a0 e8 e5 ba 00 00 00 00 00 00 00 00 00 00 00 02 |................|
  05c0: 45 0a 00 00 00 00 00 00 00 05 48 00 00 00 62 85 |E.........H...b.|
  05d0: 00 18 9e 74 a9 e0 47 5e 82 20 93 bc 7d b0 d6 31 |...t..G^. ..}..1|
  05e0: ae b0 b4 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  05f0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0600: 00 00 00 00 00 00 00 00 00 00 00 02 de 42 19 6e |.............B.n|
  0610: be e4 2e f2 84 b6 78 0a 87 cd c9 6e 8e aa b6 00 |......x....n....|
  0620: 00 00 00 00 00 00 00 00 00 00 02 48 0a 00 00 00 |...........H....|
  0630: 00 00 00 00 00 00 00 00 00 00 00 00 00          |.............|

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

  $ f --hexdump ../rev-reply.hg2
  ../rev-reply.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 2f 11 72 65 70 |HG20......./.rep|
  0010: 6c 79 3a 63 68 61 6e 67 65 67 72 6f 75 70 00 00 |ly:changegroup..|
  0020: 00 00 00 02 0b 01 06 01 69 6e 2d 72 65 70 6c 79 |........in-reply|
  0030: 2d 74 6f 31 72 65 74 75 72 6e 31 00 00 00 00 00 |-to1return1.....|
  0040: 00 00 1b 06 6f 75 74 70 75 74 00 00 00 01 00 01 |....output......|
  0050: 0b 01 69 6e 2d 72 65 70 6c 79 2d 74 6f 31 00 00 |..in-reply-to1..|
  0060: 00 64 61 64 64 69 6e 67 20 63 68 61 6e 67 65 73 |.dadding changes|
  0070: 65 74 73 0a 61 64 64 69 6e 67 20 6d 61 6e 69 66 |ets.adding manif|
  0080: 65 73 74 73 0a 61 64 64 69 6e 67 20 66 69 6c 65 |ests.adding file|
  0090: 20 63 68 61 6e 67 65 73 0a 61 64 64 65 64 20 30 | changes.added 0|
  00a0: 20 63 68 61 6e 67 65 73 65 74 73 20 77 69 74 68 | changesets with|
  00b0: 20 30 20 63 68 61 6e 67 65 73 20 74 6f 20 33 20 | 0 changes to 3 |
  00c0: 66 69 6c 65 73 0a 00 00 00 00 00 00 00 00       |files.........|

Check handling of exception during generation.
----------------------------------------------

  $ hg bundle2 --genraise > ../genfailed.hg2
  abort: Someone set up us the bomb!
  [255]

Should still be a valid bundle

  $ f --hexdump ../genfailed.hg2
  ../genfailed.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 0d 06 6f 75 74 |HG20.........out|
  0010: 70 75 74 00 00 00 00 00 00 ff ff ff ff 00 00 00 |put.............|
  0020: 48 0b 65 72 72 6f 72 3a 61 62 6f 72 74 00 00 00 |H.error:abort...|
  0030: 00 01 00 07 2d 6d 65 73 73 61 67 65 75 6e 65 78 |....-messageunex|
  0040: 70 65 63 74 65 64 20 65 72 72 6f 72 3a 20 53 6f |pected error: So|
  0050: 6d 65 6f 6e 65 20 73 65 74 20 75 70 20 75 73 20 |meone set up us |
  0060: 74 68 65 20 62 6f 6d 62 21 00 00 00 00 00 00 00 |the bomb!.......|
  0070: 00                                              |.|

And its handling on the other size raise a clean exception

  $ cat ../genfailed.hg2 | hg unbundle2
  0 unread bytes
  abort: unexpected error: Someone set up us the bomb!
  [255]


  $ cd ..
