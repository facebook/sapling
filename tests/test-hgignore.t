  $ hg init ignorerepo
  $ cd ignorerepo

Issue562: .hgignore requires newline at end:

  $ touch foo
  $ touch bar
  $ touch baz
  $ cat > makeignore.py <<EOF
  > f = open(".hgignore", "w")
  > f.write("ignore\n")
  > f.write("foo\n")
  > # No EOL here
  > f.write("bar")
  > f.close()
  > EOF

  $ python makeignore.py

Should display baz only:

  $ hg status
  ? baz

  $ rm foo bar baz .hgignore makeignore.py

  $ touch a.o
  $ touch a.c
  $ touch syntax
  $ mkdir dir
  $ touch dir/a.o
  $ touch dir/b.o
  $ touch dir/c.o

  $ hg add dir/a.o
  $ hg commit -m 0
  $ hg add dir/b.o

  $ hg status
  A dir/b.o
  ? a.c
  ? a.o
  ? dir/c.o
  ? syntax

  $ echo "*.o" > .hgignore
  $ hg status
  abort: $TESTTMP/ignorerepo/.hgignore: invalid pattern (relre): *.o (glob)
  [255]

  $ echo ".*\.o" > .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

Ensure that comments work:

  $ touch 'foo#bar' 'quux#'
#if no-windows
  $ touch 'baz\#wat'
#endif
  $ cat <<'EOF' >> .hgignore
  > # full-line comment
  >   # whitespace-only comment line
  > syntax# pattern, no whitespace, then comment
  > a.c  # pattern, then whitespace, then comment
  > baz\\# # escaped comment character
  > foo\#b # escaped comment character
  > quux\## escaped comment character at end of name
  > EOF
  $ hg status
  A dir/b.o
  ? .hgignore
  $ rm 'foo#bar' 'quux#'
#if no-windows
  $ rm 'baz\#wat'
#endif

Check it does not ignore the current directory '.':

  $ echo "^\." > .hgignore
  $ hg status
  A dir/b.o
  ? a.c
  ? a.o
  ? dir/c.o
  ? syntax

Test that patterns from ui.ignore options are read:

  $ echo > .hgignore
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ignore.other = $TESTTMP/ignorerepo/.hg/testhgignore
  > EOF
  $ echo "glob:**.o" > .hg/testhgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

empty out testhgignore
  $ echo > .hg/testhgignore

Test relative ignore path (issue4473):

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ignore.relative = .hg/testhgignorerel
  > EOF
  $ echo "glob:*.o" > .hg/testhgignorerel
  $ cd dir
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

  $ cd ..
  $ echo > .hg/testhgignorerel
  $ echo "syntax: glob" > .hgignore
  $ echo "re:.*\.o" >> .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

  $ echo "syntax: invalid" > .hgignore
  $ hg status
  $TESTTMP/ignorerepo/.hgignore: ignoring invalid syntax 'invalid' (glob)
  A dir/b.o
  ? .hgignore
  ? a.c
  ? a.o
  ? dir/c.o
  ? syntax

  $ echo "syntax: glob" > .hgignore
  $ echo "*.o" >> .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

  $ echo "relglob:syntax*" > .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? a.o
  ? dir/c.o

  $ echo "relglob:*" > .hgignore
  $ hg status
  A dir/b.o

  $ cd dir
  $ hg status .
  A b.o

  $ hg debugignore
  (?:(?:|.*/)[^/]*(?:/|$))

  $ hg debugignore b.o
  b.o is ignored
  (ignore rule in $TESTTMP/ignorerepo/.hgignore, line 1: '*') (glob)

  $ cd ..

Check patterns that match only the directory

  $ echo "^dir\$" > .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? a.o
  ? syntax

Check recursive glob pattern matches no directories (dir/**/c.o matches dir/c.o)

  $ echo "syntax: glob" > .hgignore
  $ echo "dir/**/c.o" >> .hgignore
  $ touch dir/c.o
  $ mkdir dir/subdir
  $ touch dir/subdir/c.o
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? a.o
  ? syntax
  $ hg debugignore a.c
  a.c is not ignored
  $ hg debugignore dir/c.o
  dir/c.o is ignored
  (ignore rule in $TESTTMP/ignorerepo/.hgignore, line 2: 'dir/**/c.o') (glob)

Check using 'include:' in ignore file

  $ hg purge --all --config extensions.purge=
  $ touch foo.included

  $ echo ".*.included" > otherignore
  $ hg status -I "include:otherignore"
  ? foo.included

  $ echo "include:otherignore" >> .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? otherignore

Check recursive uses of 'include:'

  $ echo "include:nested/ignore" >> otherignore
  $ mkdir nested
  $ echo "glob:*ignore" > nested/ignore
  $ hg status
  A dir/b.o

  $ cp otherignore goodignore
  $ echo "include:badignore" >> otherignore
  $ hg status
  skipping unreadable pattern file 'badignore': No such file or directory
  A dir/b.o

  $ mv goodignore otherignore

Check using 'include:' while in a non-root directory

  $ cd ..
  $ hg -R ignorerepo status
  A dir/b.o
  $ cd ignorerepo

Check including subincludes

  $ hg revert -q --all
  $ hg purge --all --config extensions.purge=
  $ echo ".hgignore" > .hgignore
  $ mkdir dir1 dir2
  $ touch dir1/file1 dir1/file2 dir2/file1 dir2/file2
  $ echo "subinclude:dir2/.hgignore" >> .hgignore
  $ echo "glob:file*2" > dir2/.hgignore
  $ hg status
  ? dir1/file1
  ? dir1/file2
  ? dir2/file1

Check including subincludes with regexs

  $ echo "subinclude:dir1/.hgignore" >> .hgignore
  $ echo "regexp:f.le1" > dir1/.hgignore

  $ hg status
  ? dir1/file2
  ? dir2/file1

Check multiple levels of sub-ignores

  $ mkdir dir1/subdir
  $ touch dir1/subdir/subfile1 dir1/subdir/subfile3 dir1/subdir/subfile4
  $ echo "subinclude:subdir/.hgignore" >> dir1/.hgignore
  $ echo "glob:subfil*3" >> dir1/subdir/.hgignore

  $ hg status
  ? dir1/file2
  ? dir1/subdir/subfile4
  ? dir2/file1

Check include subignore at the same level

  $ mv dir1/subdir/.hgignore dir1/.hgignoretwo
  $ echo "regexp:f.le1" > dir1/.hgignore
  $ echo "subinclude:.hgignoretwo" >> dir1/.hgignore
  $ echo "glob:file*2" > dir1/.hgignoretwo

  $ hg status | grep file2
  [1]
  $ hg debugignore dir1/file2
  dir1/file2 is ignored
  (ignore rule in dir2/.hgignore, line 1: 'file*2')

#if windows

Windows paths are accepted on input

  $ rm dir1/.hgignore
  $ echo "dir1/file*" >> .hgignore
  $ hg debugignore "dir1\file2"
  dir1\file2 is ignored
  (ignore rule in $TESTTMP\ignorerepo\.hgignore, line 4: 'dir1/file*')
  $ hg up -qC .

#endif
