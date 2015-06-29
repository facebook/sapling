  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert=
  > [convert]
  > hg.saverev=False
  > EOF
  $ hg init orig
  $ cd orig
  $ echo foo > foo
  $ echo bar > bar
  $ hg ci -qAm 'add foo bar' -d '0 0'
  $ echo >> foo
  $ hg ci -m 'change foo' -d '1 0'
  $ hg up -qC 0
  $ hg copy --after --force foo bar
  $ hg copy foo baz
  $ hg ci -m 'make bar and baz copies of foo' -d '2 0'
  created new head

Test that template can print all file copies (issue4362)
  $ hg log -r . --template "{file_copies % ' File: {file_copy}\n'}"
   File: bar (foo)
   File: baz (foo)

  $ hg bookmark premerge1
  $ hg merge -r 1
  merging baz and foo to baz
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge local copy' -d '3 0'
  $ hg up -C 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark premerge1)
  $ hg bookmark premerge2
  $ hg merge 2
  merging foo and baz to baz
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merge remote copy' -d '4 0'
  created new head

Make and delete some tags

  $ hg tag that
  $ hg tag --remove that
  $ hg tag this

#if execbit
  $ chmod +x baz
#else
  $ echo some other change to make sure we get a rev 5 > baz
#endif
  $ hg ci -m 'mark baz executable' -d '5 0'
  $ cd ..
  $ hg convert --datesort orig new 2>&1 | grep -v 'subversion python bindings could not be loaded'
  initializing destination new repository
  scanning source...
  sorting...
  converting...
  8 add foo bar
  7 change foo
  6 make bar and baz copies of foo
  5 merge local copy
  4 merge remote copy
  3 Added tag that for changeset 88586c4e9f02
  2 Removed tag that
  1 Added tag this for changeset c56a7f387039
  0 mark baz executable
  updating bookmarks
  $ cd new
  $ hg out ../orig
  comparing with ../orig
  searching for changes
  no changes found
  [1]
#if execbit
  $ hg bookmarks
     premerge1                 3:973ef48a98a4
     premerge2                 8:91d107c423ba
#else
Different hash because no x bit
  $ hg bookmarks
     premerge1                 3:973ef48a98a4
     premerge2                 8:3537b15eaaca
#endif

Test that redoing a convert results in an identical graph
  $ cd ../
  $ rm new/.hg/shamap
  $ hg convert --datesort orig new 2>&1 | grep -v 'subversion python bindings could not be loaded'
  scanning source...
  sorting...
  converting...
  8 add foo bar
  7 change foo
  6 make bar and baz copies of foo
  5 merge local copy
  4 merge remote copy
  3 Added tag that for changeset 88586c4e9f02
  2 Removed tag that
  1 Added tag this for changeset c56a7f387039
  0 mark baz executable
  updating bookmarks
  $ hg -R new log -G -T '{rev} {desc}'
  o  8 mark baz executable
  |
  o  7 Added tag this for changeset c56a7f387039
  |
  o  6 Removed tag that
  |
  o  5 Added tag that for changeset 88586c4e9f02
  |
  o    4 merge remote copy
  |\
  +---o  3 merge local copy
  | |/
  | o  2 make bar and baz copies of foo
  | |
  o |  1 change foo
  |/
  o  0 add foo bar
  

check shamap LF and CRLF handling

  $ cat > rewrite.py <<EOF
  > import sys
  > # Interlace LF and CRLF
  > lines = [(l.rstrip() + ((i % 2) and '\n' or '\r\n'))
  >          for i, l in enumerate(file(sys.argv[1]))]
  > file(sys.argv[1], 'wb').write(''.join(lines))
  > EOF
  $ python rewrite.py new/.hg/shamap
  $ cd orig
  $ hg up -qC 1
  $ echo foo >> foo
  $ hg ci -qm 'change foo again'
  $ hg up -qC 2
  $ echo foo >> foo
  $ hg ci -qm 'change foo again again'
  $ cd ..
  $ hg convert --datesort orig new 2>&1 | grep -v 'subversion python bindings could not be loaded'
  scanning source...
  sorting...
  converting...
  1 change foo again again
  0 change foo again
  updating bookmarks

init broken repository

  $ hg init broken
  $ cd broken
  $ echo a >> a
  $ echo b >> b
  $ hg ci -qAm init
  $ echo a >> a
  $ echo b >> b
  $ hg copy b c
  $ hg ci -qAm changeall
  $ hg up -qC 0
  $ echo bc >> b
  $ hg ci -m changebagain
  created new head
  $ HGMERGE=internal:local hg -q merge
  $ hg ci -m merge
  $ hg mv b d
  $ hg ci -m moveb

break it

  $ rm .hg/store/data/b.*
  $ cd ..
  $ hg --config convert.hg.ignoreerrors=True convert broken fixed
  initializing destination fixed repository
  scanning source...
  sorting...
  converting...
  4 init
  ignoring: data/b.i@1e88685f5dde: no match found
  3 changeall
  2 changebagain
  1 merge
  0 moveb
  $ hg -R fixed verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 5 changesets, 5 total revisions

manifest -r 0

  $ hg -R fixed manifest -r 0
  a

manifest -r tip

  $ hg -R fixed manifest -r tip
  a
  c
  d
