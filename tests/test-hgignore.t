  $ hg init

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
  abort: $TESTTMP/.hgignore: invalid pattern (relre): *.o (glob)
  [255]

  $ echo ".*\.o" > .hgignore
  $ hg status
  A dir/b.o
  ? .hgignore
  ? a.c
  ? syntax

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
  > ignore.other = $TESTTMP/.hg/testhgignore
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
  $TESTTMP/.hgignore: ignoring invalid syntax 'invalid' (glob)
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

  $ echo "include:nestedignore" >> otherignore
  $ echo "glob:*ignore" > nestedignore
  $ hg status
  A dir/b.o

  $ cp otherignore goodignore
  $ echo "include:badignore" >> otherignore
  $ hg status
  skipping unreadable pattern file 'badignore': No such file or directory
  A dir/b.o

  $ mv goodignore otherignore

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
