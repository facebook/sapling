  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if hg identify -q > /dev/null; then :
  > else
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi
  $ hg manifest | xargs "$check_code" || echo 'FAILURE IS NOT AN OPTION!!!'
  mercurial/wireproto.py:560:
   >                     yield sopener(name).read(size)
   use opener.read() instead
  FAILURE IS NOT AN OPTION!!!

  $ hg manifest | xargs "$check_code" --warnings --nolineno --per-file=0 || true
  hgext/convert/cvsps.py:0:
   >                     ui.write('Ancestors: %s\n' % (','.join(r)))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                     ui.write('Parent: %d\n' % cs.parents[0].id)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                     ui.write('Parents: %s\n' %
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >                 ui.write('Branchpoints: %s \n' % ', '.join(branchpoints))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Author: %s\n' % cs.author)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Branch: %s\n' % (cs.branch or 'HEAD'))
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Date: %s\n' % util.datestr(cs.date,
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Log:\n')
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Members: \n')
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('PatchSet %d \n' % cs.id)
   warning: unwrapped ui message
  hgext/convert/cvsps.py:0:
   >             ui.write('Tag%s: %s \n' % (['', 's'][len(cs.tags) > 1],
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >         ui.write("parent %s\n" % p)
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >         ui.write('k=%s\nv=%s\n' % (name, value))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("author %s %s %s\n" % (ctx.user(), int(date[0]), date[1]))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("branch %s\n\n" % ctx.branch())
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("committer %s %s %s\n" % (committer, int(date[0]), date[1]))
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("revision %d\n" % ctx.rev())
   warning: unwrapped ui message
  hgext/hgk.py:0:
   >     ui.write("tree %s\n" % short(ctx.changeset()[0]))
   warning: unwrapped ui message
  hgext/mq.py:0:
   >         ui.write("mq:     %s\n" % ', '.join(m))
   warning: unwrapped ui message
  hgext/patchbomb.py:0:
   >             ui.write('Subject: %s\n' % subj)
   warning: unwrapped ui message
  hgext/patchbomb.py:0:
   >         ui.write('From: %s\n' % sender)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.note('branch %s\n' % data)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.note('node %s\n' % str(data))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.note('tag %s\n' % name)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >                 ui.write("unpruned common: %s\n" % " ".join([short(n)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write("format: id, p1, p2, cset, delta base, len(delta)\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write("local is subset\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write("remote is subset\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('deltas against other : ' + fmt % pcfmt(numother,
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('deltas against p1    : ' + fmt % pcfmt(nump1, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >             ui.write('deltas against p2    : ' + fmt % pcfmt(nump2, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write("common heads: %s\n" % " ".join([short(n) for n in common]))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write("match: %s\n" % m(d[0]))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('deltas against prev  : ' + fmt % pcfmt(numprev, numdeltas))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('path %s\n' % k)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >         ui.write('uncompressed data size (min/max/avg) : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write("digraph G {\n")
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write("internal: %s %s\n" % d)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write("standard: %s\n" % util.datestr(d))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('avg chain length  : ' + fmt % avgchainlen)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('case-sensitive: %s\n' % (util.checkcase('.debugfsinfo')
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('compression ratio : ' + fmt % compratio)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('delta size (min/max/avg)             : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('exec: %s\n' % (util.checkexec(path) and 'yes' or 'no'))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('flags  : %s\n' % ', '.join(flags))
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('format : %d\n' % format)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('full revision size (min/max/avg)     : %d / %d / %d\n'
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('revision size : ' + fmt2 % totalsize)
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('revisions     : ' + fmt2 % numrevs)
   warning: unwrapped ui message
   warning: unwrapped ui message
  mercurial/commands.py:0:
   >     ui.write('symlink: %s\n' % (util.checklink(path) and 'yes' or 'no'))
   warning: unwrapped ui message
  mercurial/wireproto.py:0:
   >                     yield sopener(name).read(size)
   use opener.read() instead
  tests/autodiff.py:0:
   >         ui.write('data lost for: %s\n' % fn)
   warning: unwrapped ui message
  tests/test-convert-mtn.t:0:
   >   > function get_passphrase(keypair_id)
   don't use 'function', use old style
  tests/test-import-git.t:0:
   >   > Mc\${NkU|\`?^000jF3jhEB
   ^ must be quoted
  tests/test-import.t:0:
   >   > diff -Naur proj-orig/foo proj-new/foo
   don't use 'diff -N'
   don't use 'diff -N'
  tests/test-schemes.t:0:
   >   > z = file:\$PWD/
   don't use $PWD, use `pwd`
  tests/test-ui-color.py:0:
   > testui.warn('warning\n')
   warning: unwrapped ui message
  tests/test-ui-color.py:0:
   > testui.write('buffered\n')
   warning: unwrapped ui message
