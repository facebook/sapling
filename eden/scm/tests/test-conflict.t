#chg-compatible
#debugruntest-compatible

  $ hg init repo
  $ cd repo
  $ cat << EOF > a
  > Small Mathematical Series.
  > One
  > Two
  > Three
  > Four
  > Five
  > Hop we are done.
  > EOF
  $ hg add a
  $ hg commit -m ancestor
  $ cat << EOF > a
  > Small Mathematical Series.
  > 1
  > 2
  > 3
  > 4
  > 5
  > Hop we are done.
  > EOF
  $ hg commit -m branch1
  $ hg co 'desc(ancestor)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF > a
  > Small Mathematical Series.
  > 1
  > 2
  > 3
  > 6
  > 8
  > Hop we are done.
  > EOF
  $ hg commit -m branch2

  $ hg merge 'desc(branch1)'
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ hg id
  618808747361+c0c68e4fe667+

  $ echo "[commands]" >> $HGRCPATH
  $ echo "status.verbose=true" >> $HGRCPATH
  $ hg status
  M a
  ? a.orig
  # The repository is in an unfinished *merge* state.
  
  # Unresolved merge conflicts:
  # 
  #     a
  # 
  # To mark files as resolved:  hg resolve --mark FILE
  
  # To continue:                hg commit
  # To abort:                   hg goto --clean .    (warning: this will discard uncommitted changes)
  

  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<< working copy: 618808747361 - test: branch2
  6
  8
  =======
  4
  5
  >>>>>>> merge rev:    c0c68e4fe667 - test: branch1
  Hop we are done.

  $ hg status --config commands.status.verbose=0
  M a
  ? a.orig

Verify custom conflict markers

  $ hg up -q --clean .
  $ cat <<EOF >> .hg/hgrc
  > [ui]
  > mergemarkertemplate = '{author} {node}'
  > EOF

  $ hg merge 'desc(branch1)'
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<< working copy: test 6188087473610f9c9c11296d35620d7e0d35f796
  6
  8
  =======
  4
  5
  >>>>>>> merge rev:    test c0c68e4fe667f80c031c0e5871bcb12fae657a57
  Hop we are done.

Verify line splitting of custom conflict marker which causes multiple lines

  $ hg up -q --clean .
  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > mergemarkertemplate={author} {node}\nfoo\nbar\nbaz
  > EOF

  $ hg -q merge 'desc(branch1)'
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]

  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<< working copy: test 6188087473610f9c9c11296d35620d7e0d35f796
  6
  8
  =======
  4
  5
  >>>>>>> merge rev:    test c0c68e4fe667f80c031c0e5871bcb12fae657a57
  Hop we are done.

Verify line trimming of custom conflict marker using multi-byte characters

  $ hg up -q --clean .
  $ $PYTHON <<EOF
  > fp = open('logfile', 'wb')
  > fp.write(b'12345678901234567890123456789012345678901234567890' +
  >          b'1234567890') # there are 5 more columns for 80 columns
  > 
  > # 2 x 4 = 8 columns, but 3 x 4 = 12 bytes
  > fp.write(b'\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88')
  > 
  > fp.close()
  > EOF
  $ hg add logfile
  $ hg --encoding utf-8 commit --logfile logfile

  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > mergemarkertemplate={desc|firstline}
  > EOF

  $ hg -q --encoding utf-8 merge 'desc(branch1)'
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]

  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<< working copy: 1234567890123456789012345678901234567890123456789012345...
  6
  8
  =======
  4
  5
  >>>>>>> merge rev:    branch1
  Hop we are done.

Verify basic conflict markers

  $ hg up -q --clean 'desc(branch2)'
  $ printf "\n[ui]\nmergemarkers=basic\n" >> .hg/hgrc

  $ hg merge 'desc(branch1)'
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<< working copy
  6
  8
  =======
  4
  5
  >>>>>>> merge rev
  Hop we are done.

internal:merge3

  $ hg up -q --clean .

  $ hg merge 'desc(branch1)' --tool internal:merge3
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ cat a
  Small Mathematical Series.
  <<<<<<< working copy
  1
  2
  3
  6
  8
  ||||||| base
  One
  Two
  Three
  Four
  Five
  =======
  1
  2
  3
  4
  5
  >>>>>>> merge rev
  Hop we are done.

Add some unconflicting changes on each head, to make sure we really
are merging, unlike :local and :other

  $ hg up -C
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "e0693e20f496: 123456789012345678901234567890123456789012345678901234567890\xe3\x81\x82\xe3\x81\x84\xe3\x81\x86\xe3\x81\x88" (esc)
  1 other heads for branch "default"
  $ printf "\n\nEnd of file\n" >> a
  $ hg ci -m "Add some stuff at the end"
  $ hg up -r 'desc(branch1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ printf "Start of file\n\n\n" > tmp
  $ cat a >> tmp
  $ mv tmp a
  $ hg ci -m "Add some stuff at the beginning"

Now test :merge-other and :merge-local

  $ hg merge
  merging a
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg resolve --tool :merge-other a
  merging a
  (no more unresolved files)
  $ cat a
  Start of file
  
  
  Small Mathematical Series.
  1
  2
  3
  6
  8
  Hop we are done.
  
  
  End of file

  $ hg up -C
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  updated to "18b51d585961: Add some stuff at the beginning"
  1 other heads for branch "default"
  $ hg merge --tool :merge-local
  merging a
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat a
  Start of file
  
  
  Small Mathematical Series.
  1
  2
  3
  4
  5
  Hop we are done.
  
  
  End of file

internal:mergediff

  $ hg co -C 'desc(branch1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat << EOF > a
  > Small Mathematical Series.
  > 1
  > 2
  > 3
  > 4
  > 4.5
  > 5
  > Hop we are done.
  > EOF
  $ hg co -m 'desc(branch2)' -t internal:mergediff
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ cat a
  Small Mathematical Series.
  1
  2
  3
  <<<<<<<
  ------- base
  +++++++ working copy
   4
  +4.5
   5
  ======= destination
  6
  8
  >>>>>>>
  Hop we are done.
Test the same thing as above but modify a bit more so we instead get the working
copy in full and the diff from base to destination.
  $ hg co -C 'desc(branch1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat << EOF > a
  > Small Mathematical Series.
  > 1
  > 2
  > 3.5
  > 4.5
  > 5.5
  > Hop we are done.
  > EOF
  $ hg co -m 'desc(branch2)' -t internal:mergediff
  merging a
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  [1]
  $ cat a
  Small Mathematical Series.
  1
  2
  <<<<<<<
  ======= working copy
  3.5
  4.5
  5.5
  ------- base
  +++++++ destination
   3
  -4
  -5
  +6
  +8
  >>>>>>>
  Hop we are done.
