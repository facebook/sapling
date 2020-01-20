#chg-compatible

  $ disable treemanifest
This test is dedicated to test the bundle2 container format

It test multiple existing parts to test different feature of the container. You
probably do not need to touch this test unless you change the binary encoding
of the bundle2 format itself.

Create an extension to test bundle2 API

  $ cat > bundle2.py << EOF
  > """A small extension to test bundle2 implementation
  > 
  > This extension allows detailed testing of the various bundle2 API and
  > behaviors.
  > """
  > import gc
  > import os
  > import sys
  > from edenscm.mercurial import util
  > from edenscm.mercurial import bundle2
  > from edenscm.mercurial import scmutil
  > from edenscm.mercurial import discovery
  > from edenscm.mercurial import changegroup
  > from edenscm.mercurial import error
  > from edenscm.mercurial import obsolete
  > from edenscm.mercurial import pycompat
  > from edenscm.mercurial import registrar
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
  > command = registrar.command(cmdtable)
  > 
  > ELEPHANTSSONG = b"""Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
  > Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  > Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko."""
  > assert len(ELEPHANTSSONG) == 178 # future test say 178 bytes, trust it.
  > 
  > @bundle2.parthandler(b'test:song')
  > def songhandler(op, part):
  >     """handle a "test:song" bundle2 part, printing the lyrics on stdin"""
  >     op.ui.write(b'The choir starts singing:\n')
  >     verses = 0
  >     for line in part.read().split(b'\n'):
  >         op.ui.write(b'    %s\n' % line)
  >         verses += 1
  >     op.records.add(b'song', {b'verses': verses})
  > 
  > @bundle2.parthandler(b'test:ping')
  > def pinghandler(op, part):
  >     op.ui.write(b'received ping request (id %i)\n' % part.id)
  >     if op.reply is not None and b'ping-pong' in op.reply.capabilities:
  >         op.ui.write_err(b'replying to ping request (id %i)\n' % part.id)
  >         op.reply.newpart(b'test:pong', [(b'in-reply-to', b'%d' % part.id)],
  >                          mandatory=False)
  > 
  > @bundle2.parthandler(b'test:debugreply')
  > def debugreply(op, part):
  >     """print data about the capacity of the bundle reply"""
  >     if op.reply is None:
  >         op.ui.write(b'debugreply: no reply\n')
  >     else:
  >         op.ui.write(b'debugreply: capabilities:\n')
  >         for cap in sorted(op.reply.capabilities):
  >             op.ui.write(b"debugreply:     '%s'\n" % cap)
  >             for val in op.reply.capabilities[cap]:
  >                 op.ui.write(b"debugreply:         '%s'\n" % val)
  > 
  > @command(b'bundle2',
  >          [(b'', b'param', [], b'stream level parameter'),
  >           (b'', b'unknown', False, b'include an unknown mandatory part in the bundle'),
  >           (b'', b'unknownparams', False, b'include an unknown part parameters in the bundle'),
  >           (b'', b'parts', False, b'include some arbitrary parts to the bundle'),
  >           (b'', b'reply', False, b'produce a reply bundle'),
  >           (b'', b'genraise', False, b'includes a part that raise an exception during generation'),
  >           (b'', b'timeout', False, b'emulate a timeout during bundle generation'),
  >           (b'r', b'rev', [], b'includes those changeset in the bundle'),
  >           (b'', b'compress', b'', b'compress the stream'),],
  >          b'[OUTPUTFILE]')
  > def cmdbundle2(ui, repo, path=None, **opts):
  >     """write a bundle2 container on standard output"""
  >     bundler = bundle2.bundle20(ui)
  >     for p in opts['param']:
  >         p = p.split(b'=', 1)
  >         try:
  >             bundler.addparam(*p)
  >         except ValueError as exc:
  >             raise error.Abort('%s' % exc)
  > 
  >     if opts['compress']:
  >         bundler.setcompression(opts['compress'])
  > 
  >     if opts['reply']:
  >         capsstring = b'ping-pong\nelephants=babar,celeste\ncity%3D%21=celeste%2Cville'
  >         bundler.newpart(b'replycaps', data=capsstring)
  > 
  >     revs = opts['rev']
  >     if 'rev' in opts:
  >         revs = scmutil.revrange(repo, opts['rev'])
  >         if revs:
  >             # very crude version of a changegroup part creation
  >             bundled = repo.revs('%ld::%ld', revs, revs)
  >             headmissing = [c.node() for c in repo.set('heads(%ld)', revs)]
  >             headcommon  = [c.node() for c in repo.set('parents(%ld) - %ld', revs, revs)]
  >             outgoing = discovery.outgoing(repo, headcommon, headmissing)
  >             cg = changegroup.makechangegroup(repo, outgoing, b'02',
  >                                              b'test:bundle2')
  >             part = bundler.newpart(b'changegroup', data=cg.getchunks(),
  >                                    mandatory=False)
  >             part.addparam('version', b'02')
  > 
  >     if opts['parts']:
  >        bundler.newpart(b'test:empty', mandatory=False)
  >        # add a second one to make sure we handle multiple parts
  >        bundler.newpart(b'test:empty', mandatory=False)
  >        bundler.newpart(b'test:song', data=ELEPHANTSSONG, mandatory=False)
  >        bundler.newpart(b'test:debugreply', mandatory=False)
  >        mathpart = bundler.newpart(b'test:math')
  >        mathpart.addparam(b'pi', b'3.14')
  >        mathpart.addparam(b'e', b'2.72')
  >        mathpart.addparam(b'cooking', b'raw', mandatory=False)
  >        mathpart.data = b'42'
  >        mathpart.mandatory = False
  >        # advisory known part with unknown mandatory param
  >        bundler.newpart(b'test:song', [(b'randomparam', b'')], mandatory=False)
  >     if opts['unknown']:
  >        bundler.newpart(b'test:unknown', data=b'some random content')
  >     if opts['unknownparams']:
  >        bundler.newpart(b'test:song', [(b'randomparams', b'')])
  >     if opts['parts']:
  >        bundler.newpart(b'test:ping', mandatory=False)
  >     if opts['genraise']:
  >        def genraise():
  >            yield b'first line\n'
  >            raise RuntimeError('Someone set up us the bomb!')
  >        bundler.newpart(b'output', data=genraise(), mandatory=False)
  > 
  >     if path is None:
  >        file = pycompat.stdout
  >     else:
  >         file = open(path, 'wb')
  > 
  >     if opts['timeout']:
  >         bundler.newpart(b'test:song', data=ELEPHANTSSONG, mandatory=False)
  >         for idx, junk in enumerate(bundler.getchunks()):
  >             ui.write(b'%d chunk\n' % idx)
  >             if idx > 4:
  >                 # This throws a GeneratorExit inside the generator, which
  >                 # can cause problems if the exception-recovery code is
  >                 # too zealous. It's important for this test that the break
  >                 # occur while we're in the middle of a part.
  >                 break
  >         gc.collect()
  >         ui.write(b'fake timeout complete.\n')
  >         return
  >     try:
  >         for chunk in bundler.getchunks():
  >             file.write(chunk)
  >     except RuntimeError as exc:
  >         raise error.Abort(exc)
  >     finally:
  >         file.flush()
  > 
  > @command(b'unbundle2', [], b'')
  > def cmdunbundle2(ui, repo, replypath=None):
  >     """process a bundle2 stream from stdin on the current repo"""
  >     try:
  >         tr = None
  >         lock = repo.lock()
  >         tr = repo.transaction(b'processbundle')
  >         try:
  >             unbundler = bundle2.getunbundler(ui, pycompat.stdin)
  >             op = bundle2.processbundle(repo, unbundler, lambda: tr)
  >             tr.close()
  >         except error.BundleValueError as exc:
  >             raise error.Abort('missing support for %s' % exc)
  >         except error.PushRaced as exc:
  >             raise error.Abort('push race: %s' % exc)
  >     finally:
  >         if tr is not None:
  >             tr.release()
  >         lock.release()
  >         remains = pycompat.stdin.read()
  >         ui.write(b'%i unread bytes\n' % len(remains))
  >     if op.records[b'song']:
  >         totalverses = sum(r[b'verses'] for r in op.records[b'song'])
  >         ui.write(b'%i total verses sung\n' % totalverses)
  >     for rec in op.records[b'changegroup']:
  >         ui.write(b'addchangegroup return: %i\n' % rec[b'return'])
  >     if op.reply is not None and replypath is not None:
  >         with open(replypath, 'wb') as file:
  >             for chunk in op.reply.getchunks():
  >                 file.write(chunk)
  > 
  > @command(b'statbundle2', [], b'')
  > def cmdstatbundle2(ui, repo):
  >     """print statistic on the bundle2 container read from stdin"""
  >     unbundler = bundle2.getunbundler(ui, pycompat.stdin)
  >     try:
  >         params = unbundler.params
  >     except error.BundleValueError as exc:
  >        raise error.Abort(b'unknown parameters: %s' % exc)
  >     ui.write(b'options count: %i\n' % len(params))
  >     for key in sorted(params):
  >         ui.write(b'- %s\n' % key)
  >         value = params[key]
  >         if value is not None:
  >             ui.write(b'    %s\n' % value)
  >     count = 0
  >     for p in unbundler.iterparts():
  >         count += 1
  >         ui.write(b'  :%s:\n' % p.type)
  >         ui.write(b'    mandatory: %i\n' % len(p.mandatoryparams))
  >         ui.write(b'    advisory: %i\n' % len(p.advisoryparams))
  >         ui.write(b'    payload: %i bytes\n' % len(p.read()))
  >     ui.write(b'parts count:   %i\n' % count)
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > bundle2=$TESTTMP/bundle2.py
  > [experimental]
  > evolution.createmarkers=True
  > [ui]
  > ssh=$PYTHON "$TESTDIR/dummyssh"
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

  $ hg bundle --all --type v1 ../bundle.hg --config format.allowbundle1=True
  devel-warn: using deprecated bundlev1 format
   at: */changegroup.py:* (makechangegroup) (glob)
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
  bundle2-input: ignoring unknown parameter e|! 7/
  bundle2-input: ignoring unknown parameter simple
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
  abort: non letter first character: 42babar
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
  bundle2-output-part: "test:math" (advisory) (params: 2 mandatory 1 advisory) 2 bytes payload
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
  bundle2-input: found a handler for part test:song
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
  bundle2-input: found a handler for part test:debugreply
  bundle2-input-part: "test:debugreply" (advisory) supported
  debugreply: no reply
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 43
  bundle2-input: part type: "test:math"
  bundle2-input: part id: "4"
  bundle2-input: part parameters: 3
  bundle2-input: ignoring unsupported advisory part test:math
  bundle2-input-part: "test:math" (advisory) (params: 2 mandatory 1 advisory) unsupported-type
  bundle2-input: payload chunk size: 2
  bundle2-input: payload chunk size: 0
  bundle2-input-part: total payload size 2
  bundle2-input: part header size: 29
  bundle2-input: part type: "test:song"
  bundle2-input: part id: "5"
  bundle2-input: part parameters: 1
  bundle2-input: found a handler for part test:song
  bundle2-input: ignoring unsupported advisory part test:song - randomparam
  bundle2-input-part: "test:song" (advisory) (params: 1 mandatory) unsupported-params (randomparam)
  bundle2-input: payload chunk size: 0
  bundle2-input: part header size: 16
  bundle2-input: part type: "test:ping"
  bundle2-input: part id: "6"
  bundle2-input: part parameters: 0
  bundle2-input: found a handler for part test:ping
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

Support for changegroup
===================================

  $ hg unbundle $TESTDIR/bundles/rebase.hg
  adding changesets
  adding manifests
  adding file changes
  added 8 changesets with 7 changes to 7 files

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
  bundle2-output-part: "changegroup" (advisory) (params: 1 mandatory) streamed payload
  bundle2-output: part 0: "changegroup"
  bundle2-output: header chunk size: 29
  progress: bundling: 1/4 changesets (25.00%)
  progress: bundling: 2/4 changesets (50.00%)
  progress: bundling: 3/4 changesets (75.00%)
  progress: bundling: 4/4 changesets (100.00%)
  progress: bundling (end)
  progress: bundling: 1/4 manifests (25.00%)
  progress: bundling: 2/4 manifests (50.00%)
  progress: bundling: 3/4 manifests (75.00%)
  progress: bundling: 4/4 manifests (100.00%)
  progress: bundling (end)
  progress: bundling: D 1/3 files (33.33%)
  progress: bundling: E 2/3 files (66.67%)
  progress: bundling: H 3/3 files (100.00%)
  progress: bundling (end)
  bundle2-output: payload chunk size: 1915
  bundle2-output: closing payload chunk
  bundle2-output: end of bundle

  $ f --hexdump ../rev.hg2
  ../rev.hg2:
  0000: 48 47 32 30 00 00 00 00 00 00 00 1d 0b 63 68 61 |HG20.........cha|
  0010: 6e 67 65 67 72 6f 75 70 00 00 00 00 01 00 07 02 |ngegroup........|
  0020: 76 65 72 73 69 6f 6e 30 32 00 00 07 7b 00 00 00 |version02...{...|
  0030: de 32 af 76 86 d4 03 cf 45 b5 d9 5f 2d 70 ce be |.2.v....E.._-p..|
  0040: a5 87 ac 80 6a 5f dd d9 89 57 c8 a5 4a 4d 43 6d |....j_...W..JMCm|
  0050: fe 1d a9 d8 7f 21 a1 b9 7b 00 00 00 00 00 00 00 |.....!..{.......|
  0060: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0070: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0080: 00 32 af 76 86 d4 03 cf 45 b5 d9 5f 2d 70 ce be |.2.v....E.._-p..|
  0090: a5 87 ac 80 6a 00 00 00 00 00 00 00 00 00 00 00 |....j...........|
  00a0: 6a 36 65 31 66 34 63 34 37 65 63 62 35 33 33 66 |j6e1f4c47ecb533f|
  00b0: 66 64 30 63 38 65 35 32 63 64 63 38 38 61 66 62 |fd0c8e52cdc88afb|
  00c0: 36 63 64 33 39 65 32 30 63 0a 4e 69 63 6f 6c 61 |6cd39e20c.Nicola|
  00d0: 73 20 44 75 6d 61 7a 65 74 20 3c 6e 69 63 64 75 |s Dumazet <nicdu|
  00e0: 6d 7a 2e 63 6f 6d 6d 69 74 73 40 67 6d 61 69 6c |mz.commits@gmail|
  00f0: 2e 63 6f 6d 3e 0a 31 33 30 34 31 36 39 38 38 38 |.com>.1304169888|
  0100: 20 2d 37 32 30 30 0a 44 0a 0a 44 00 00 00 de 95 | -7200.D..D.....|
  0110: 20 ee a7 81 bc ca 16 c1 e1 5a cc 0b a1 43 35 a0 | ........Z...C5.|
  0120: e8 e5 ba cd 01 0b 8c d9 98 f3 98 1a 5a 81 15 f9 |............Z...|
  0130: 4f 8d a4 ab 50 60 89 00 00 00 00 00 00 00 00 00 |O...P`..........|
  0140: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0150: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 95 |................|
  0160: 20 ee a7 81 bc ca 16 c1 e1 5a cc 0b a1 43 35 a0 | ........Z...C5.|
  0170: e8 e5 ba 00 00 00 00 00 00 00 00 00 00 00 6a 34 |..............j4|
  0180: 64 65 63 65 39 63 38 32 36 66 36 39 34 39 30 35 |dece9c826f694905|
  0190: 30 37 62 39 38 63 36 33 38 33 61 33 30 30 39 62 |07b98c6383a3009b|
  01a0: 32 39 35 38 33 37 64 0a 4e 69 63 6f 6c 61 73 20 |295837d.Nicolas |
  01b0: 44 75 6d 61 7a 65 74 20 3c 6e 69 63 64 75 6d 7a |Dumazet <nicdumz|
  01c0: 2e 63 6f 6d 6d 69 74 73 40 67 6d 61 69 6c 2e 63 |.commits@gmail.c|
  01d0: 6f 6d 3e 0a 31 33 30 34 31 36 39 38 38 38 20 2d |om>.1304169888 -|
  01e0: 37 32 30 30 0a 45 0a 0a 45 00 00 00 dc ee a1 37 |7200.E..E......7|
  01f0: 46 79 9a 9e 0b fd 88 f2 9d 3c 2e 9d c9 38 9f 52 |Fy.......<...8.R|
  0200: 4f 24 b6 38 7c 8c 8c ae 37 17 88 80 f3 fa 95 de |O$.8|...7.......|
  0210: d3 cb 1c f7 85 95 20 ee a7 81 bc ca 16 c1 e1 5a |...... ........Z|
  0220: cc 0b a1 43 35 a0 e8 e5 ba 00 00 00 00 00 00 00 |...C5...........|
  0230: 00 00 00 00 00 00 00 00 00 00 00 00 00 ee a1 37 |...............7|
  0240: 46 79 9a 9e 0b fd 88 f2 9d 3c 2e 9d c9 38 9f 52 |Fy.......<...8.R|
  0250: 4f 00 00 00 00 00 00 00 00 00 00 00 68 33 36 35 |O...........h365|
  0260: 62 39 33 64 35 37 66 64 66 34 38 31 34 65 32 62 |b93d57fdf4814e2b|
  0270: 35 39 31 31 64 36 62 61 63 66 66 32 62 31 32 30 |5911d6bacff2b120|
  0280: 31 34 34 34 31 0a 4e 69 63 6f 6c 61 73 20 44 75 |14441.Nicolas Du|
  0290: 6d 61 7a 65 74 20 3c 6e 69 63 64 75 6d 7a 2e 63 |mazet <nicdumz.c|
  02a0: 6f 6d 6d 69 74 73 40 67 6d 61 69 6c 2e 63 6f 6d |ommits@gmail.com|
  02b0: 3e 0a 31 33 30 34 31 36 39 38 38 38 20 2d 37 32 |>.1304169888 -72|
  02c0: 30 30 0a 0a 47 00 00 00 de 02 de 42 19 6e be e4 |00..G......B.n..|
  02d0: 2e f2 84 b6 78 0a 87 cd c9 6e 8e aa b6 24 b6 38 |....x....n...$.8|
  02e0: 7c 8c 8c ae 37 17 88 80 f3 fa 95 de d3 cb 1c f7 ||...7...........|
  02f0: 85 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0300: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0310: 00 00 00 00 00 00 00 00 00 02 de 42 19 6e be e4 |...........B.n..|
  0320: 2e f2 84 b6 78 0a 87 cd c9 6e 8e aa b6 00 00 00 |....x....n......|
  0330: 00 00 00 00 00 00 00 00 6a 38 62 65 65 34 38 65 |........j8bee48e|
  0340: 64 63 37 33 31 38 35 34 31 66 63 30 30 31 33 65 |dc7318541fc0013e|
  0350: 65 34 31 62 30 38 39 32 37 36 61 38 63 32 34 62 |e41b089276a8c24b|
  0360: 66 0a 4e 69 63 6f 6c 61 73 20 44 75 6d 61 7a 65 |f.Nicolas Dumaze|
  0370: 74 20 3c 6e 69 63 64 75 6d 7a 2e 63 6f 6d 6d 69 |t <nicdumz.commi|
  0380: 74 73 40 67 6d 61 69 6c 2e 63 6f 6d 3e 0a 31 33 |ts@gmail.com>.13|
  0390: 30 34 31 36 39 38 38 38 20 2d 37 32 30 30 0a 48 |04169888 -7200.H|
  03a0: 0a 0a 48 00 00 00 00 00 00 00 9f 6e 1f 4c 47 ec |..H........n.LG.|
  03b0: b5 33 ff d0 c8 e5 2c dc 88 af b6 cd 39 e2 0c 66 |.3....,.....9..f|
  03c0: a5 a0 18 17 fd f5 23 9c 27 38 02 b5 b7 61 8d 05 |......#.'8...a..|
  03d0: 1c 89 e4 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  03e0: 00 00 00 00 00 00 00 66 a5 a0 18 17 fd f5 23 9c |.......f......#.|
  03f0: 27 38 02 b5 b7 61 8d 05 1c 89 e4 32 af 76 86 d4 |'8...a.....2.v..|
  0400: 03 cf 45 b5 d9 5f 2d 70 ce be a5 87 ac 80 6a 00 |..E.._-p......j.|
  0410: 00 00 81 00 00 00 81 00 00 00 2b 44 00 63 33 66 |..........+D.c3f|
  0420: 31 63 61 32 39 32 34 63 31 36 61 31 39 62 30 36 |1ca2924c16a19b06|
  0430: 35 36 61 38 34 39 30 30 65 35 30 34 65 35 62 30 |56a84900e504e5b0|
  0440: 61 65 63 32 64 0a 00 00 00 9f 4d ec e9 c8 26 f6 |aec2d.....M...&.|
  0450: 94 90 50 7b 98 c6 38 3a 30 09 b2 95 83 7d 00 7d |..P{..8:0....}.}|
  0460: 8c 9d 88 84 13 25 f5 c6 b0 63 71 b3 5b 4e 8a 2b |.....%...cq.[N.+|
  0470: 1a 83 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0480: 00 00 00 00 00 00 00 7d 8c 9d 88 84 13 25 f5 c6 |.......}.....%..|
  0490: b0 63 71 b3 5b 4e 8a 2b 1a 83 95 20 ee a7 81 bc |.cq.[N.+... ....|
  04a0: ca 16 c1 e1 5a cc 0b a1 43 35 a0 e8 e5 ba 00 00 |....Z...C5......|
  04b0: 00 2b 00 00 00 2b 00 00 00 2b 45 00 39 63 36 66 |.+...+...+E.9c6f|
  04c0: 64 30 33 35 30 61 36 63 30 64 30 63 34 39 64 34 |d0350a6c0d0c49d4|
  04d0: 61 39 63 35 30 31 37 63 66 30 37 30 34 33 66 35 |a9c5017cf07043f5|
  04e0: 34 65 35 38 0a 00 00 00 9f 36 5b 93 d5 7f df 48 |4e58.....6[....H|
  04f0: 14 e2 b5 91 1d 6b ac ff 2b 12 01 44 41 28 a5 84 |.....k..+..DA(..|
  0500: c6 5e f1 21 f8 9e b6 6a b7 d0 bc 15 3d 80 99 e7 |.^.!...j....=...|
  0510: ce 4d ec e9 c8 26 f6 94 90 50 7b 98 c6 38 3a 30 |.M...&...P{..8:0|
  0520: 09 b2 95 83 7d 28 a5 84 c6 5e f1 21 f8 9e b6 6a |....}(...^.!...j|
  0530: b7 d0 bc 15 3d 80 99 e7 ce ee a1 37 46 79 9a 9e |....=......7Fy..|
  0540: 0b fd 88 f2 9d 3c 2e 9d c9 38 9f 52 4f 00 00 00 |.....<...8.RO...|
  0550: 2b 00 00 00 2b 00 00 00 2b 45 00 39 63 36 66 64 |+...+...+E.9c6fd|
  0560: 30 33 35 30 61 36 63 30 64 30 63 34 39 64 34 61 |0350a6c0d0c49d4a|
  0570: 39 63 35 30 31 37 63 66 30 37 30 34 33 66 35 34 |9c5017cf07043f54|
  0580: 65 35 38 0a 00 00 00 9f 8b ee 48 ed c7 31 85 41 |e58.......H..1.A|
  0590: fc 00 13 ee 41 b0 89 27 6a 8c 24 bf 28 a5 84 c6 |....A..'j.$.(...|
  05a0: 5e f1 21 f8 9e b6 6a b7 d0 bc 15 3d 80 99 e7 ce |^.!...j....=....|
  05b0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  05c0: 00 00 00 00 28 a5 84 c6 5e f1 21 f8 9e b6 6a b7 |....(...^.!...j.|
  05d0: d0 bc 15 3d 80 99 e7 ce 02 de 42 19 6e be e4 2e |...=......B.n...|
  05e0: f2 84 b6 78 0a 87 cd c9 6e 8e aa b6 00 00 00 56 |...x....n......V|
  05f0: 00 00 00 56 00 00 00 2b 48 00 38 35 30 30 31 38 |...V...+H.850018|
  0600: 39 65 37 34 61 39 65 30 34 37 35 65 38 32 32 30 |9e74a9e0475e8220|
  0610: 39 33 62 63 37 64 62 30 64 36 33 31 61 65 62 30 |93bc7db0d631aeb0|
  0620: 62 34 0a 00 00 00 00 00 00 00 05 44 00 00 00 76 |b4.........D...v|
  0630: c3 f1 ca 29 24 c1 6a 19 b0 65 6a 84 90 0e 50 4e |...)$.j..ej...PN|
  0640: 5b 0a ec 2d 00 00 00 00 00 00 00 00 00 00 00 00 |[..-............|
  0650: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0660: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0670: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0680: 32 af 76 86 d4 03 cf 45 b5 d9 5f 2d 70 ce be a5 |2.v....E.._-p...|
  0690: 87 ac 80 6a 00 00 00 00 00 00 00 00 00 00 00 02 |...j............|
  06a0: 44 0a 00 00 00 00 00 00 00 05 45 00 00 00 76 9c |D.........E...v.|
  06b0: 6f d0 35 0a 6c 0d 0c 49 d4 a9 c5 01 7c f0 70 43 |o.5.l..I....|.pC|
  06c0: f5 4e 58 00 00 00 00 00 00 00 00 00 00 00 00 00 |.NX.............|
  06d0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  06e0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  06f0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 95 |................|
  0700: 20 ee a7 81 bc ca 16 c1 e1 5a cc 0b a1 43 35 a0 | ........Z...C5.|
  0710: e8 e5 ba 00 00 00 00 00 00 00 00 00 00 00 02 45 |...............E|
  0720: 0a 00 00 00 00 00 00 00 05 48 00 00 00 76 85 00 |.........H...v..|
  0730: 18 9e 74 a9 e0 47 5e 82 20 93 bc 7d b0 d6 31 ae |..t..G^. ..}..1.|
  0740: b0 b4 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0750: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0760: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|
  0770: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 02 de |................|
  0780: 42 19 6e be e4 2e f2 84 b6 78 0a 87 cd c9 6e 8e |B.n......x....n.|
  0790: aa b6 00 00 00 00 00 00 00 00 00 00 00 02 48 0a |..............H.|
  07a0: 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 |................|

  $ hg debugbundle ../rev.hg2
  Stream params: {}
  changegroup -- {version: 02}
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

Test compression
================

Simple case where it just work: GZ
----------------------------------

  $ hg bundle2 --compress GZ --rev '8+7+5+4' ../rev.hg2.bz
#if common-zlib
  $ f --hexdump ../rev.hg2.bz
  ../rev.hg2.bz:
  0000: 48 47 32 30 00 00 00 0e 43 6f 6d 70 72 65 73 73 |HG20....Compress|
  0010: 69 6f 6e 3d 47 5a 78 9c a5 54 7f 68 55 55 1c bf |ion=GZx..T.hUU..|
  0020: 7b 21 d2 9d f5 47 e6 4f 1c 3d 69 a5 f1 dc 38 3f |{!...G.O.=i...8?|
  0030: ef 3d 27 2c 5a 7b af bd 44 e7 e8 8f 12 07 da 3d |.=',Z{..D......=|
  0040: e7 9e 3b df 6b ef bd e5 b6 47 a9 c3 2d b7 dc ea |..;.k....G..-...|
  0050: 19 86 0e a6 b8 e9 a0 11 2b d1 87 b5 27 c5 30 24 |........+...'.0$|
  0060: 5a d3 9a 0d b2 68 60 ab 44 a9 60 8d 54 a6 15 99 |Z....h`.D.`.T...|
  0070: dd 17 6b 8c b8 63 a3 f7 85 ef 1f f7 9c ef fd 7e |..k..c.........~|
  0080: be 9f f3 fd 7e 3f 9a a6 15 e4 cb 6d 56 bc 4a 55 |....~?.....mV.JU|
  0090: 6d 4f d4 d7 68 ae e5 69 f3 7d 49 b5 bd 36 92 88 |mO..h..i.}I..6..|
  00a0: 03 a4 69 f3 77 ba 67 a3 e8 64 f2 d5 8b 77 7d 11 |..i.w.g..d...w}.|
  00b0: ea 1b d9 5a 54 73 e1 4c cf de e3 8d d1 ad df 8e |...ZTs.L........|
  00c0: b4 3d 37 d8 b3 6e 43 69 ec af 82 de 6f 76 af ec |.=7..nCi....ov..|
  00d0: fe 20 1b 3b 27 f3 ca 37 ed 3a 6a 28 e8 10 49 4c |. .;'..7.:j(..IL|
  00e0: 25 05 c5 d8 71 6c 20 99 a2 48 da 92 31 cb 11 86 |%...ql ..H..1...|
  00f0: b4 31 57 08 48 bd 3c 22 13 d5 56 ad 3f 58 1f b3 |.1W.H.<"..V.?X..|
  0100: 76 a8 3a ff da 78 44 da f5 b1 1d c5 32 11 8b 45 |v.:..xD.....2..E|
  0110: ea 6a 9f a8 8a 59 91 ea ec d7 e3 3a c4 80 40 83 |.j...Y.....:..@.|
  0120: 33 c6 fc 45 26 02 40 0f ea 7a 30 cb ae dd 3f fe |3..E&.@..z0...?.|
  0130: 76 53 ff f9 c5 67 7f d8 fc 79 7e 77 29 3d f6 d3 |vS...g...y~w)=..|
  0140: d5 0f 87 f2 f2 53 23 1d 37 3a 96 6f 6e 5a f4 fb |.....S#.7:.onZ..|
  0150: c6 7d 6f bd 5b f1 7c db 5c d9 79 e5 9b ce 8e d8 |.}o.[.|.\.y.....|
  0160: 4a 2a 2e 19 32 1c 83 13 0e 28 30 05 67 d2 c0 0c |J*..2....(0.g...|
  0170: 5b 18 00 2e 10 a7 0c 9b 76 6e ec 42 ba 1e 72 d1 |[.......vn.B..r.|
  0180: 2e 8d 77 9b 4f bd 7c b8 2b ff 76 eb f5 ce b5 c5 |..w.O.|.+.v.....|
  0190: 9d e7 d8 d1 67 36 16 66 d8 ae 54 ea 84 b9 a4 b5 |....g6.f..T.....|
  01a0: f1 c6 1f ed a3 5f 7e b6 e2 56 cb 2c 55 4f 99 57 |....._~..V.,UO.W|
  01b0: be 69 d7 db b0 41 05 c7 36 35 1d db 21 0c 12 85 |.i...A..65..!...|
  01c0: 04 e5 10 da 86 b0 a4 e3 20 01 11 80 84 10 98 1b |........ .......|
  01d0: 3b bd 2c db 39 df e8 93 cb e2 67 ae 14 5f 6f ce |;.,.9.....g.._o.|
  01e0: bc a4 ef 1d 3a 17 7f e3 9d 8c 17 b7 b9 76 ce 2b |....:........v.+|
  01f0: df f4 ce 31 a1 14 61 ca 96 26 86 8c 12 e8 48 00 |...1..a..&....H.|
  0200: 20 76 cf a0 00 8c 23 d3 b0 98 44 44 38 b9 71 0b | v....#...DD8.q.|
  0210: eb 7a 78 12 f1 68 fc 81 f5 65 63 7d f8 ce f0 e0 |.zx..h...ec}....|
  0220: d5 35 97 5a 4f 66 86 f8 e5 05 4e cf b1 a5 4b 6e |.5.ZOf....N...Kn|
  0230: 4f 3c 78 64 15 f3 f5 9d b6 f6 cd 5b d1 76 c5 8b |O<xd.......[.v..|
  0240: 8d 57 dc 0c 9b d7 34 e9 81 a0 26 b1 03 a5 85 38 |.W....4...&....8|
  0250: 22 12 1a 16 e4 02 18 d4 e5 e5 ce 29 50 14 10 45 |"..........)P..E|
  0260: 05 b0 94 44 b6 9e 2d 6f c3 d8 cf 83 0f df 3c b8 |...D..-o......<.|
  0270: bf 62 67 c7 00 7b 14 dc 7d aa 7d 4f 83 d6 90 ea |.bg..{..}.}O....|
  0280: 6c 6d 5e f8 d0 c4 40 5a be f8 5e 65 f9 6b 81 e5 |lm^...@Z..^e.k..|
  0290: 7b 3c 5f db 23 6e 86 21 0c fc eb 21 8d 4b c3 95 |{<_.#n.!...!.K..|
  02a0: 03 4c 81 65 48 e0 ea 02 e1 36 b1 b8 a4 00 9a d2 |.L.eH....6......|
  02b0: 01 26 20 d8 a1 6e 91 ec 9f f2 8c ca 03 5f ed fe |.& ..n......._..|
  02c0: 2e 7c ff e5 be 37 0b 5e 38 7e 27 70 5f 5e b0 64 |.|...7.^8~'p_^.d|
  02d0: 75 4f f3 c0 96 6b 2b 7f eb ca 44 4f 0f f7 2f 7a |uO...k+...DO../z|
  02e0: ac f1 d0 8f 17 bc 68 78 c5 cd 30 fb ff b7 bc d7 |......hx..0.....|
  02f0: c7 c3 bf 7c 0a 5b 4a fe d4 16 8e 97 a4 db 56 45 |...|.[J.......VE|
  0300: 53 85 1f 79 c1 7a 3d 9e 57 dc 0c e3 fb ec a4 07 |S..y.z=.W.......|
  0310: c2 1a a3 ee c0 32 ae 4c b7 28 05 88 49 15 43 08 |.....2.L.(..I.C.|
  0320: 70 2c a4 69 0b 60 1b 18 5a 4a 00 41 f4 49 94 79 |p,.i.`..ZJ.A.I.y|
  0330: 59 81 4c 7e 7c ed fc 23 85 67 a3 cb d2 2a da bc |Y.L~|..#.g...*..|
  0340: ff de 8a f2 4a 7d ac 68 ae 2b e5 65 b3 c8 bf 2f |....J}.h.+.e.../|
  0350: 38 05 9f 55 b0 e4 91 c4 30 d5 ab ef 59 f0 f4 c5 |8..U....0...Y...|
  0360: de 4f f2 76 fd 5a 53 3a 51 be 29 17 f8 59 94 ce |.O.v.ZS:Q.)..Y..|
  0370: 17 9a 82 cf ae 61 b2 45 5b da 55 d7 fb 7d d9 96 |.....a.E[.U..}..|
  0380: 57 fc 07 fa 1b d2 5f c3 13 e9 f7 73 81 9f 45 64 |W....._....s..Ed|
  0390: 7c 61 fd bf 7f fc 0d 55 9f 36 e8                ||a.....U.6.|
#endif
  $ hg debugbundle ../rev.hg2.bz
  Stream params: {Compression: GZ}
  changegroup -- {version: 02}
      32af7686d403cf45b5d95f2d70cebea587ac806a
      9520eea781bcca16c1e15acc0ba14335a0e8e5ba
      eea13746799a9e0bfd88f29d3c2e9dc9389f524f
      02de42196ebee42ef284b6780a87cdc96e8eaab6
  $ hg unbundle ../rev.hg2.bz
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 3 files
Simple case where it just work: BZ
----------------------------------

  $ hg bundle2 --compress BZ --rev '8+7+5+4' ../rev.hg2.bz
  $ f --hexdump ../rev.hg2.bz
  ../rev.hg2.bz:
  0000: 48 47 32 30 00 00 00 0e 43 6f 6d 70 72 65 73 73 |HG20....Compress|
  0010: 69 6f 6e 3d 42 5a 42 5a 68 39 31 41 59 26 53 59 |ion=BZBZh91AY&SY|
  0020: 51 dc b5 bb 00 00 14 ff ff fa bf 7f f6 ef ef 7f |Q...............|
  0030: f7 7f f7 d1 d9 ff ff ff 7e ff ff 6e 77 e6 bd df |........~..nw...|
  0040: b5 ab ff cf 67 f6 e7 7b f7 d0 03 5b d4 41 c8 c3 |....g..{...[.A..|
  0050: a1 0d 4a 80 00 00 00 00 03 40 00 00 00 03 40 00 |..J......@....@.|
  0060: 06 8d 00 00 32 68 00 00 00 00 00 00 00 00 00 03 |....2h..........|
  0070: d4 1a a5 3d 27 a9 a3 40 00 00 00 00 06 8d 0d 00 |...='..@........|
  0080: 00 00 00 34 00 00 00 00 00 00 00 00 00 00 00 01 |...4............|
  0090: a0 00 03 54 c0 40 d3 4d 21 11 a0 00 00 19 00 1a |...T.@.M!.......|
  00a0: 34 19 1a 0d 0d 00 00 1a 00 06 46 80 00 00 00 00 |4.........F.....|
  00b0: 03 40 00 34 68 00 00 04 1a 06 80 64 19 06 81 a0 |.@.4h......d....|
  00c0: 68 c4 c8 d0 34 00 62 00 34 0c 21 a3 40 d0 00 00 |h...4.b.4.!.@...|
  00d0: 03 40 64 68 d0 00 00 00 0d 19 19 32 62 68 06 4d |.@dh.......2bh.M|
  00e0: 18 25 28 51 93 26 82 06 8d 01 a0 64 1e a3 4d 03 |.%(Q.&.....d..M.|
  00f0: 6a 69 b5 3c 84 69 a6 9a 1a 30 4d 1a 34 64 00 d3 |ji.<.i...0M.4d..|
  0100: 43 f5 40 0d 32 34 d3 40 31 0d a8 d0 68 3d 43 4c |C.@.24.@1...h=CL|
  0110: 9e 93 d4 68 d0 d3 46 13 27 a2 69 e5 32 a4 75 c9 |...h..F.'.i.2.u.|
  0120: fc 87 37 97 44 53 19 1d ba c4 6a 69 f4 95 3c c9 |..7.DS....ji..<.|
  0130: da 56 38 00 ad 11 c7 99 3a 29 c8 29 50 e1 1b 2a |.V8.....:).)P..*|
  0140: 8e 8d 53 26 39 05 35 8b 60 cc 12 4d 41 04 22 d4 |..S&9.5.`..MA.".|
  0150: 10 c4 8a 54 44 68 a3 21 41 8a 88 ba 82 16 d1 ce |...TDh.!A.......|
  0160: f3 01 2e 8d a3 2a dd 03 c4 06 d8 59 dc 33 80 60 |.....*.....Y.3.`|
  0170: 05 0b 23 6c 39 c2 1c e9 45 20 13 86 21 08 09 45 |..#l9...E ..!..E|
  0180: 9c 7a 9b 0b 50 06 66 82 bf df 42 b9 31 92 37 54 |.z..P.f...B.1.7T|
  0190: f5 b0 40 d6 a8 57 1c ec 1a bc 42 80 d6 9e 01 8d |..@..W....B.....|
  01a0: 18 48 57 34 1c 96 e3 0e e3 fe 7c 68 54 85 48 56 |.HW4......|hT.HV|
  01b0: d8 21 5b 80 0f 88 54 71 29 5f 49 38 3d 3c 24 d7 |.![...Tq)_I8=<$.|
  01c0: be 7a 9f 57 97 86 08 1d f1 50 bc fd ef 02 a8 34 |.z.W.....P.....4|
  01d0: 01 5e 0b a5 08 2e 38 0d 98 38 6f c7 5e 0e 0e 08 |.^....8..8o.^...|
  01e0: 56 3a 50 60 1a 42 b1 84 25 64 b0 b2 ac d9 ac 2c |V:P`.B..%d.....,|
  01f0: 19 e7 20 06 46 11 d3 50 80 71 6f 40 09 30 59 37 |.. .F..P.qo@.0Y7|
  0200: 25 3c 34 51 1b cb 58 eb 40 4c 6a 86 16 14 8b 62 |%<4Q..X.@Lj....b|
  0210: d2 d5 2b 17 33 14 f4 20 2e a5 e6 0d eb e7 9a 2c |..+.3.. .......,|
  0220: 61 d5 cb 13 3c 12 8d 00 12 2c a0 ce 95 d9 49 82 |a...<....,....I.|
  0230: 03 88 43 25 85 4f bf 08 83 d1 03 e2 13 c6 0a c4 |..C%.O..........|
  0240: 3e 1b 12 03 12 53 b3 64 e0 4c 10 40 fc 40 75 48 |>....S.d.L.@.@uH|
  0250: c7 9d 98 16 d9 21 1d d0 b1 4c 01 9b 31 3d 77 13 |.....!...L..1=w.|
  0260: 10 6a 92 cd 80 b5 14 4d 80 78 12 84 2a 33 3c e8 |.j.....M.x..*3<.|
  0270: 13 60 2a 03 f9 c2 1a ea 95 0d e0 3f 56 e1 82 da |.`*........?V...|
  0280: fa e5 ce 1c 80 15 87 58 7c 35 f0 66 bf 9a 20 aa |.......X|5.f.. .|
  0290: 72 da 05 40 1c 80 1d c0 39 eb 32 ba 02 99 02 f6 |r..@....9.2.....|
  02a0: 92 05 96 0a 94 93 88 a0 14 58 a0 51 3d 20 2f 90 |.........X.Q= /.|
  02b0: 49 db f2 4b 97 2e 58 ea 35 36 ed ae 0b 3d 0c 2e |I..K..X.56...=..|
  02c0: 19 80 27 06 21 1f 96 5c 53 60 c1 0c 02 1b 1a eb |..'.!..\S`......|
  02d0: c8 80 4f 40 69 b0 36 01 d0 1c 12 4a 36 d0 d6 0b |..O@i.6....J6...|
  02e0: 03 24 1a 34 7a 66 78 0a 1a 40 0a 05 60 bf 2a 7e |.$.4zfx..@..`.*~|
  02f0: f9 81 be a2 08 cd 82 54 eb 64 8e 47 94 80 63 c3 |.......T.d.G..c.|
  0300: 5e 32 44 21 04 0b 94 52 9d 8e bd 17 48 03 44 28 |^2D!...R....H.D(|
  0310: 37 87 03 92 00 f4 85 25 a2 00 49 71 09 47 89 2b |7......%..Iq.G.+|
  0320: 4c 8e 94 07 58 43 a0 3b 21 9c 3c b5 34 3f 22 97 |L...XC.;!.<.4?".|
  0330: 2b a6 da ac 70 07 7c 25 af 48 61 22 cc 50 85 a9 |+...p.|%.Ha".P..|
  0340: 33 d9 7c 6f 38 09 ad ba 8e 05 a0 10 c1 31 18 1e |3.|o8........1..|
  0350: e4 bb 84 45 59 4d cd 85 4b 6c c8 7a c0 ca 05 a0 |...EYM..Kl.z....|
  0360: 34 05 81 50 9f d3 2d 13 b9 09 e0 74 11 40 90 b6 |4..P..-....t.@..|
  0370: 9c 28 57 db 60 60 44 63 5a 07 28 19 2c 0c c2 ae |.(W.``DcZ.(.,...|
  0380: 84 23 43 15 b0 d2 dd 02 86 44 3d 49 16 10 e1 77 |.#C......D=I...w|
  0390: 96 d4 92 05 03 a1 9f 8b ab 7d 7d f9 0f 68 5c 0c |.........}}..h\.|
  03a0: 4b 6f c2 0f 36 35 62 8c 09 9c 20 a2 0c 49 57 2a |Ko..65b... ..IW*|
  03b0: aa 0b 06 8c 3d 85 31 81 01 3e 53 13 96 a3 92 4a |....=.1..>S....J|
  03c0: 26 e4 ea 1d 72 77 38 76 25 c1 d5 d9 d0 e4 19 fd |&...rw8v%.......|
  03d0: 05 ac 0d 33 1a f4 05 0f ed a0 a6 5c 0e c2 72 05 |...3.......\..r.|
  03e0: f6 22 29 03 04 f3 26 79 40 36 04 d2 6c 9d 96 06 |.")...&y@6..l...|
  03f0: 08 0d 91 6d 0f a5 ab af b0 3e 0d f8 c5 07 9b a9 |...m.....>......|
  0400: fb f0 c1 bf 0b dc 81 aa 5c 1c f3 0a 5f 2d 84 8f |........\..._-..|
  0410: dc be 1c 78 ed e8 8b 80 7c 1a 72 90 1b 85 1b 72 |...x....|.r....r|
  0420: 58 2c 6c d0 b2 0c 17 a8 57 04 b8 34 3f 48 36 7c |X,l.....W..4?H6||
  0430: f4 88 0e f1 02 01 fc 87 a0 4f 48 6f 50 b7 c4 29 |.........OHoP..)|
  0440: 50 bd e3 47 83 f4 76 0a 4a 82 f5 e0 f2 74 c3 0c |P..G..v.J....t..|
  0450: 38 c6 b9 2c d8 3f 6d fd 99 7a a1 1e 0a 53 17 2c |8..,.?m..z...S.,|
  0460: bb 48 58 e1 59 cc 82 93 50 10 7e 17 88 49 8b f9 |.HX.Y...P.~..I..|
  0470: de 97 b7 4f ec a4 b9 b4 e8 54 1c 00 a9 b0 0b 91 |...O.....T......|
  0480: d6 78 1b 19 3f c0 a0 70 18 03 a2 9c 47 99 25 88 |.x..?..p....G.%.|
  0490: be b7 d1 ff 17 72 45 38 50 90 51 dc b5 bb       |.....rE8P.Q...|
  $ hg debugbundle ../rev.hg2.bz
  Stream params: {Compression: BZ}
  changegroup -- {version: 02}
      32af7686d403cf45b5d95f2d70cebea587ac806a
      9520eea781bcca16c1e15acc0ba14335a0e8e5ba
      eea13746799a9e0bfd88f29d3c2e9dc9389f524f
      02de42196ebee42ef284b6780a87cdc96e8eaab6
  $ hg unbundle ../rev.hg2.bz
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 3 files

unknown compression while unbundling
-----------------------------

  $ hg bundle2 --param Compression=FooBarUnknown --rev '8+7+5+4' ../rev.hg2.bz
  $ cat ../rev.hg2.bz | hg statbundle2
  abort: unknown parameters: Stream Parameter - Compression='FooBarUnknown'
  [255]
  $ hg unbundle ../rev.hg2.bz
  abort: ../rev.hg2.bz: unknown bundle feature, Stream Parameter - Compression='FooBarUnknown'
  (see https://mercurial-scm.org/wiki/BundleFeature for more information)
  [255]

  $ cd ..
