
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
  > from mercurial import bundle2
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > 
  > @command('bundle2', [], '')
  > def cmdbundle2(ui, repo):
  >     """write a bundle2 container on standard ouput"""
  >     bundle = bundle2.bundle20()
  >     for chunk in bundle.getchunks():
  >         ui.write(chunk)
  > 
  > @command('unbundle2', [], '')
  > def cmdunbundle2(ui, repo):
  >     """read a bundle2 container from standard input"""
  >     unbundler = bundle2.unbundle20(sys.stdin)
  >     ui.write('options count: %i\n' % len(unbundler.params))
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

Test simple generation of empty bundle

  $ hg bundle2
  HG20\x00\x00\x00\x00 (no-eol) (esc)

Test parsing of an empty bundle

  $ hg bundle2 | hg unbundle2
  options count: 0
  parts count:   0
