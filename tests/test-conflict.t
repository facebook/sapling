  $ hg init
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
  $ hg co 0
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
  created new head

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ hg id
  618808747361+c0c68e4fe667+ tip

  $ cat a
  Small Mathematical Series.
  <<<<<<< local: 618808747361 - test: branch2
  1
  2
  3
  6
  8
  =======
  1
  2
  3
  4
  5
  >>>>>>> other: c0c68e4fe667  - test: branch1
  Hop we are done.

  $ hg status
  M a
  ? a.orig

Verify custom conflict markers

  $ hg up -q --clean .
  $ printf "\n[ui]\nmergemarkertemplate={author} {rev}\n" >> .hg/hgrc

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ cat a
  Small Mathematical Series.
  <<<<<<< local: test 2
  1
  2
  3
  6
  8
  =======
  1
  2
  3
  4
  5
  >>>>>>> other: test 1
  Hop we are done.

Verify line splitting of custom conflict marker which causes multiple lines

  $ hg up -q --clean .
  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > mergemarkertemplate={author} {rev}\nfoo\nbar\nbaz
  > EOF

  $ hg -q merge 1
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]

  $ cat a
  Small Mathematical Series.
  <<<<<<< local: test 2
  1
  2
  3
  6
  8
  =======
  1
  2
  3
  4
  5
  >>>>>>> other: test 1
  Hop we are done.

Verify line trimming of custom conflict marker using multi-byte characters

  $ hg up -q --clean .
  $ python <<EOF
  > fp = open('logfile', 'w')
  > fp.write('12345678901234567890123456789012345678901234567890' +
  >          '1234567890') # there are 5 more columns for 80 columns
  > 
  > # 2 x 4 = 8 columns, but 3 x 4 = 12 bytes
  > fp.write(u'\u3042\u3044\u3046\u3048'.encode('utf-8'))
  > 
  > fp.close()
  > EOF
  $ hg add logfile
  $ hg --encoding utf-8 commit --logfile logfile

  $ cat >> .hg/hgrc <<EOF
  > [ui]
  > mergemarkertemplate={desc|firstline}
  > EOF

  $ hg -q --encoding utf-8 merge 1
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  [1]

  $ cat a
  Small Mathematical Series.
  <<<<<<< local: 123456789012345678901234567890123456789012345678901234567890\xe3\x81\x82... (esc)
  1
  2
  3
  6
  8
  =======
  1
  2
  3
  4
  5
  >>>>>>> other: branch1
  Hop we are done.

Verify basic conflict markers

  $ hg up -q --clean 2
  $ printf "\n[ui]\nmergemarkers=basic\n" >> .hg/hgrc

  $ hg merge 1
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ cat a
  Small Mathematical Series.
  <<<<<<< local
  1
  2
  3
  6
  8
  =======
  1
  2
  3
  4
  5
  >>>>>>> other
  Hop we are done.

internal:merge3

  $ hg up -q --clean .

  $ hg merge 1 --tool internal:merge3
  merging a
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ cat a
  Small Mathematical Series.
  <<<<<<< local
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
  >>>>>>> other
  Hop we are done.
