#debugruntest-compatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ disable treemanifest
  $ configure dummyssh
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
  > from sapling import util
  > from sapling import bundle2
  > from sapling import scmutil
  > from sapling import discovery
  > from sapling import changegroup
  > from sapling import error
  > from sapling import obsolete
  > from sapling import pycompat
  > from sapling import registrar
  > 
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > ELEPHANTSSONG = b"""Patali Dirapata, Cromda Cromda Ripalo, Pata Pata, Ko Ko Ko
  > Bokoro Dipoulito, Rondi Rondi Pepino, Pata Pata, Ko Ko Ko
  > Emana Karassoli, Loucra Loucra Ponponto, Pata Pata, Ko Ko Ko."""
  > assert len(ELEPHANTSSONG) == 178 # future test say 178 bytes, trust it.
  > 
  > @bundle2.parthandler('test:song')
  > def songhandler(op, part):
  >     """handle a "test:song" bundle2 part, printing the lyrics on stdin"""
  >     op.ui.write('The choir starts singing:\n')
  >     verses = 0
  >     for line in part.read().split(b'\n'):
  >         op.ui.write('    %s\n' % pycompat.decodeutf8(line))
  >         verses += 1
  >     op.records.add('song', {'verses': verses})
  > 
  > @bundle2.parthandler('test:ping')
  > def pinghandler(op, part):
  >     op.ui.write('received ping request (id %i)\n' % part.id)
  >     if op.reply is not None and 'ping-pong' in op.reply.capabilities:
  >         op.ui.write_err('replying to ping request (id %i)\n' % part.id)
  >         op.reply.newpart('test:pong', [('in-reply-to', '%d' % part.id)],
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
  >             op.ui.write("debugreply:     '%s'\n" % cap)
  >             for val in op.reply.capabilities[cap]:
  >                 op.ui.write("debugreply:         '%s'\n" % val)
  > 
  > @command('bundle2',
  >          [('', 'param', [], 'stream level parameter'),
  >           ('', 'unknown', False, 'include an unknown mandatory part in the bundle'),
  >           ('', 'unknownparams', False, 'include an unknown part parameters in the bundle'),
  >           ('', 'parts', False, 'include some arbitrary parts to the bundle'),
  >           ('', 'reply', False, 'produce a reply bundle'),
  >           ('', 'genraise', False, 'includes a part that raise an exception during generation'),
  >           ('', 'timeout', False, 'emulate a timeout during bundle generation'),
  >           ('r', 'rev', [], 'includes those changeset in the bundle'),
  >           ('', 'compress', '', 'compress the stream'),],
  >          b'[OUTPUTFILE]')
  > def cmdbundle2(ui, repo, path=None, **opts):
  >     """write a bundle2 container on standard output"""
  >     bundler = bundle2.bundle20(ui)
  >     for p in opts['param']:
  >         p = p.split('=', 1)
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
  >         bundler.newpart('replycaps', data=capsstring)
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
  >             cg = changegroup.makechangegroup(repo, outgoing, '02',
  >                                              'test:bundle2')
  >             part = bundler.newpart('changegroup', data=cg.getchunks(),
  >                                    mandatory=False)
  >             part.addparam('version', '02')
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
  >        mathpart.data = b'42'
  >        mathpart.mandatory = False
  >        # advisory known part with unknown mandatory param
  >        bundler.newpart('test:song', [('randomparam', '')], mandatory=False)
  >     if opts['unknown']:
  >        bundler.newpart('test:unknown', data=b'some random content')
  >     if opts['unknownparams']:
  >        bundler.newpart('test:song', [('randomparams', '')])
  >     if opts['parts']:
  >        bundler.newpart('test:ping', mandatory=False)
  >     if opts['genraise']:
  >        def genraise():
  >            yield b'first line\n'
  >            raise RuntimeError('Someone set up us the bomb!')
  >        bundler.newpart('output', data=genraise(), mandatory=False)
  > 
  >     if path is None:
  >        file = pycompat.stdout
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
  >         gc.collect()
  >         ui.write('fake timeout complete.\n')
  >         return
  >     try:
  >         for chunk in bundler.getchunks():
  >             file.write(chunk)
  >     except RuntimeError as exc:
  >         raise error.Abort(exc)
  >     finally:
  >         file.flush()
  > 
  > @command('unbundle2', [], '')
  > def cmdunbundle2(ui, repo, replypath=None):
  >     """process a bundle2 stream from stdin on the current repo"""
  >     try:
  >         tr = None
  >         lock = repo.lock()
  >         tr = repo.transaction('processbundle')
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
  >         ui.write('%i unread bytes\n' % len(remains))
  >     if op.records['song']:
  >         totalverses = sum(r['verses'] for r in op.records['song'])
  >         ui.write('%i total verses sung\n' % totalverses)
  >     for rec in op.records['changegroup']:
  >         ui.write('addchangegroup return: %i\n' % rec['return'])
  >     if op.reply is not None and replypath is not None:
  >         with open(replypath, 'wb') as file:
  >             for chunk in op.reply.getchunks():
  >                 file.write(chunk)
  > 
  > @command('statbundle2', [], '')
  > def cmdstatbundle2(ui, repo):
  >     """print statistic on the bundle2 container read from stdin"""
  >     unbundler = bundle2.getunbundler(ui, pycompat.stdin)
  >     try:
  >         params = unbundler.params
  >     except error.BundleValueError as exc:
  >        raise error.Abort('unknown parameters: %s' % exc)
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
  > evolution.createmarkers=True
  > [ui]
  > logtemplate={node|short} {phase} {author} {bookmarks} {desc|firstline}
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

  $ hg log -G
  o  02de42196ebe draft Nicolas Dumazet <nicdumz.commits@gmail.com>  H
  │
  │ o  eea13746799a draft Nicolas Dumazet <nicdumz.commits@gmail.com>  G
  ╭─┤
  o │  24b6387c8c8c draft Nicolas Dumazet <nicdumz.commits@gmail.com>  F
  │ │
  │ o  9520eea781bc draft Nicolas Dumazet <nicdumz.commits@gmail.com>  E
  ├─╯
  │ o  32af7686d403 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  D
  │ │
  │ o  5fddd98957c8 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  C
  │ │
  │ o  42ccdea3bb16 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  B
  ├─╯
  o  cd010b8cd998 draft Nicolas Dumazet <nicdumz.commits@gmail.com>  A
  
  @  3903775176ed draft test  a
  

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
  bundle2-output: payload chunk size: * (glob)
  bundle2-output: closing payload chunk
  bundle2-output: end of bundle

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

with reply

  $ hg bundle2 --rev '8+7+5+4' --reply ../rev-rr.hg2
  $ hg unbundle2 ../rev-reply.hg2 < ../rev-rr.hg2
  0 unread bytes
  addchangegroup return: 1

Check handling of exception during generation.
----------------------------------------------

  $ hg bundle2 --genraise > ../genfailed.hg2
  abort: Someone set up us the bomb!
  [255]

Should still be a valid bundle

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
Simple case where it just work: BZ
----------------------------------

  $ hg bundle2 --compress BZ --rev '8+7+5+4' ../rev.hg2.bz
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
