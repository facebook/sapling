#chg-compatible
#debugruntest-compatible

  $ configure modern
  $ enable rebase
  $ setconfig merge.word-merge=1

Successful word merge:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=That is a word.\n
  > |/  # B/A=This IS a sentence.\n
  > A   # A/A=This is a sentence.\n
  > EOS
  $ hg rebase -r $C -d $B -q
  $ hg cat -r tip A
  That IS a word.

Unsuccessful. Still show conflicts at line boundary:

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=Those are one kind of sentences.\n
  > |/  # B/A=That is one sentence.\n
  > A   # A/A=This is a sentence.\n
  > EOS
  $ hg rebase -r $C -d $B -q
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  <<<<<<< dest:   952e28cce513 - test: B
  That is one sentence.
  =======
  Those are one kind of sentences.
  >>>>>>> source: 5f8b40280adf - test: C

Partially successful at the second conflict region.

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=Bar.\n\nThis is a second line.\n
  > |/  # B/A=Foo.\n\nThat is the second line.\n
  > A   # A/A=First line.\n\nThis is the second line.\n
  > EOS
  $ hg rebase -r $C -d $B -q
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  <<<<<<< dest:   f32bfbad7819 - test: B
  Foo.
  =======
  Bar.
  >>>>>>> source: 141643ae4b3c - test: C
  
  That is a second line.

Conflicted case. Example is from a sparse related test.

  $ newrepo
  $ drawdag <<'EOS'
  > B C # C/A=[include]\n*.py\n*.txt\n
  > |/  # B/A=[include]\n*.html\n
  > A   # A/A=[include]\n*.py\n
  > EOS
  $ hg rebase -r $C -d $B -q
  warning: 1 conflicts while merging A! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ cat A
  [include]
  <<<<<<< dest:   fbfdba3e838e - test: B
  *.html
  =======
  *.py
  *.txt
  >>>>>>> source: f4337c9b9462 - test: C
