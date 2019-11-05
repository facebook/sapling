  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=
  > rebase=
  > EOF

Set up the repository with some simple files.
This is coupled with the files dictionary in
scm/hg/tests/fake-biggrep-client.py
  $ hg init repo
  $ cd repo
  $ mkdir grepdir
  $ cd grepdir
  $ echo 'foobarbaz' > grepfile1
  $ echo 'foobarboo' > grepfile2
  $ mkdir subdir1
  $ echo 'foobar_subdir' > subdir1/subfile1
  $ mkdir subdir2
  $ echo 'foobar_dirsub' > subdir2/subfile2
  $ hg add grepfile1
  $ hg add grepfile2
  $ hg add subdir1/subfile1
  $ hg add subdir2/subfile2
  $ hg commit -m "Added some files"
  $ echo 'foobarbazboo' > untracked1

Make sure grep finds patterns in tracked files, and excludes untracked files
  $ hg grep -n foobar
  grepfile1:1:foobarbaz
  grepfile2:1:foobarboo
  subdir1/subfile1:1:foobar_subdir
  subdir2/subfile2:1:foobar_dirsub
  $ hg grep -n barbaz
  grepfile1:1:foobarbaz
  $ hg grep -n barbaz .
  grepfile1:1:foobarbaz

Test searching in subdirectories, from the repository root
  $ hg grep -n foobar subdir1
  subdir1/subfile1:1:foobar_subdir
  $ hg grep -n foobar sub*
  subdir1/subfile1:1:foobar_subdir
  subdir2/subfile2:1:foobar_dirsub

Test searching in a sibling subdirectory, using a relative path
  $ cd subdir1
  $ hg grep -n foobar ../subdir2
  ../subdir2/subfile2:1:foobar_dirsub
  $ hg grep -n foobar
  subfile1:1:foobar_subdir
  $ hg grep -n foobar .
  subfile1:1:foobar_subdir
  $ cd ..

Test mercurial file patterns
  $ hg grep -n foobar 'glob:*rep*'
  grepfile1:1:foobarbaz
  grepfile2:1:foobarboo

Test using alternative grep commands
  $ hg grep -i FooBarB
  grepfile1:foobarbaz
  grepfile2:foobarboo
#if osx
  $ hg grep FooBarB
  [1]
#else
  $ hg grep FooBarB
  [123]
#endif
  $ hg grep --config grep.command='grep -i' FooBarB
  grepfile1:foobarbaz
  grepfile2:foobarboo
  $ hg grep --config grep.command='echo searching' FooBarB subdir1
  searching * -- subdir1/subfile1 (glob)
  $ hg grep --config grep.command='echo foo ; false' FooBarB subdir2
  foo ; false * -- subdir2/subfile2 (glob)

Test --include flag
  $ hg grep --include '**/*file1' -n foobar
  grepfile1:1:foobarbaz
  subdir1/subfile1:1:foobar_subdir
  $ hg grep -I '**/*file1' -n foobar
  grepfile1:1:foobarbaz
  subdir1/subfile1:1:foobar_subdir

Test --exclude flag
  $ hg grep --exclude '**/*file1' -n foobar
  grepfile2:1:foobarboo
  subdir2/subfile2:1:foobar_dirsub
  $ hg grep -X '**/*file1' -n foobar
  grepfile2:1:foobarboo
  subdir2/subfile2:1:foobar_dirsub

Test --include and --exclude flags together
  $ hg grep --include '**/*file1' --exclude '**/grepfile1' -n foobar
  subdir1/subfile1:1:foobar_subdir
  $ hg grep -I '**/*file1' -X '**/grepfile1' -n foobar
  subdir1/subfile1:1:foobar_subdir

#if symlink no-osx
Test symlinks
  $ echo file_content > target_file
  $ ln -s target_file sym_link
  $ hg add sym_link
  $ hg grep file_content
  [123]
#endif

Test with context
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > --color=off \
  > -n foobar | sort
  grepfile1:1:foobarbaz_bg
  grepfile2:1:foobarboo_bg
  subdir1/subfile1:1:foobar_subdir_bg
  subdir2/subfile2:1:foobar_dirsub_bg

Test basic biggrep client in subdir1
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > --cwd subdir1 foobar | sort
  subfile1:foobar_subdir_bg

Test basic biggrep client with subdir2 matcher
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > foobar subdir2 | sort
  subdir2/subfile2:foobar_dirsub_bg

Test biggrep searching in a sibling subdirectory, using a relative path
  $ cd subdir1
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > foobar ../subdir2 -n | sort
  ../subdir2/subfile2:1:foobar_dirsub_bg
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > -n foobar | sort
  subfile1:1:foobar_subdir_bg
  $ hg grep --config grep.biggrepclient=$TESTDIR/fake-biggrep-client.py \
  > --config grep.usebiggrep=True --config grep.biggrepcorpus=fake \
  > -n foobar . | sort
  subfile1:1:foobar_subdir_bg
  $ cd ..
