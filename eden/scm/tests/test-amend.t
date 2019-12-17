#chg-compatible

#chg-compatible

#testcases obsstore-off obsstore-on

  $ cat << EOF >> $HGRCPATH
  > [extensions]
  > amend=
  > [diff]
  > git=1
  > EOF

#if obsstore-on
  $ setconfig experimental.evolution=createmarkers
#else
  $ setconfig experimental.evolution=
#endif

Basic amend

  $ hg init repo1
  $ cd repo1
  $ hg debugdrawdag <<'EOS'
  > B
  > |
  > A
  > EOS

  $ hg update B -q
  $ echo 2 >> B

  $ hg amend
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/112478962961-7e959a55-amend.hg (obsstore-off !)
#if obsstore-off
  $ hg log -p -G --hidden -T '{rev} {node|short} {desc}\n'
  @  1 be169c7e8dbe B
  |  diff --git a/B b/B
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/B
  |  @@ -0,0 +1,1 @@
  |  +B2
  |
  o  0 426bada5c675 A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A
     \ No newline at end of file
  
#else
  $ hg log -p -G --hidden -T '{rev} {node|short} {desc}\n'
  @  2 be169c7e8dbe B
  |  diff --git a/B b/B
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/B
  |  @@ -0,0 +1,1 @@
  |  +B2
  |
  | x  1 112478962961 B
  |/   diff --git a/B b/B
  |    new file mode 100644
  |    --- /dev/null
  |    +++ b/B
  |    @@ -0,0 +1,1 @@
  |    +B
  |    \ No newline at end of file
  |
  o  0 426bada5c675 A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A
     \ No newline at end of file
  
#endif

Nothing changed

  $ hg amend
  nothing changed
  [1]

  $ hg amend -d "0 0"
  nothing changed
  [1]

  $ hg amend -d "Thu Jan 01 00:00:00 1970 UTC"
  nothing changed
  [1]

Matcher and metadata options

  $ echo 3 > C
  $ echo 4 > D
  $ hg add C D
  $ hg amend -m NEWMESSAGE -I C
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/be169c7e8dbe-7684ddc5-amend.hg (obsstore-off !)
  $ hg log -r . -T '{node|short} {desc} {files}\n'
  c7ba14d9075b NEWMESSAGE B C
  $ echo 5 > E
  $ rm C
  $ hg amend -d '2000 1000' -u 'Foo <foo@example.com>' -A C D
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/c7ba14d9075b-b3e76daa-amend.hg (obsstore-off !)
  $ hg log -r . -T '{node|short} {desc} {files} {author} {date}\n'
  14f6c4bcc865 NEWMESSAGE B D Foo <foo@example.com> 2000.01000

Amend with editor

  $ cat > $TESTTMP/prefix.sh <<'EOF'
  > printf 'EDITED: ' > $TESTTMP/msg
  > cat "$1" >> $TESTTMP/msg
  > mv $TESTTMP/msg "$1"
  > EOF
  $ chmod +x $TESTTMP/prefix.sh

  $ HGEDITOR="sh $TESTTMP/prefix.sh" hg amend --edit
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/14f6c4bcc865-6591f15d-amend.hg (obsstore-off !)
  $ hg log -r . -T '{node|short} {desc}\n'
  298f085230c3 EDITED: NEWMESSAGE
  $ HGEDITOR="sh $TESTTMP/prefix.sh" hg amend -e -m MSG
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/298f085230c3-d81a6ad3-amend.hg (obsstore-off !)
  $ hg log -r . -T '{node|short} {desc}\n'
  974f07f28537 EDITED: MSG

  $ echo FOO > $TESTTMP/msg
  $ hg amend -l $TESTTMP/msg -m BAR
  abort: options --message and --logfile are mutually exclusive
  [255]
  $ hg amend -l $TESTTMP/msg
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/974f07f28537-edb6470a-amend.hg (obsstore-off !)
  $ hg log -r . -T '{node|short} {desc}\n'
  507be9bdac71 FOO

Interactive mode

  $ touch F G
  $ hg add F G
  $ cat <<EOS | hg amend -i --config ui.interactive=1
  > y
  > n
  > EOS
  diff --git a/F b/F
  new file mode 100644
  examine changes to 'F'? [Ynesfdaq?] y
  
  diff --git a/G b/G
  new file mode 100644
  examine changes to 'G'? [Ynesfdaq?] n
  
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/507be9bdac71-c8077452-amend.hg (obsstore-off !)
  $ hg log -r . -T '{files}\n'
  B D F

Amend in the middle of a stack

  $ hg init $TESTTMP/repo2
  $ cd $TESTTMP/repo2
  $ hg debugdrawdag <<'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ hg update -q B
  $ echo 2 >> B
  $ hg amend
  warning: orphaned descendants detected, not stripping 112478962961 (obsstore-off !)
  hint[amend-restack]: descendants of 112478962961 are left behind - use 'hg restack' to rebase them
  hint[hint-ack]: use 'hg hint --ack amend-restack' to silence these hints

#if obsstore-on

  $ hg log -T '{rev} {node|short} {desc}\n' -G
  @  3 be169c7e8dbe B
  |
  | o  2 26805aba1e60 C
  | |
  | x  1 112478962961 B
  |/
  o  0 426bada5c675 A
  
#endif

Cannot amend public changeset

  $ hg phase -r A --public
  $ hg update -C -q A
  $ hg amend -m AMEND
  abort: cannot amend public changesets
  [255]

Amend a merge changeset

  $ hg init $TESTTMP/repo3
  $ cd $TESTTMP/repo3
  $ hg debugdrawdag <<'EOS'
  >   C
  >  /|
  > A B
  > EOS
  $ hg update -q C
  $ hg amend -m FOO
  saved backup bundle to $TESTTMP/repo3/.hg/strip-backup/a35c07e8a2a4-15ff4612-amend.hg (obsstore-off !)
  $ hg log -G -T '{desc}\n'
  @    FOO
  |\
  | o  B
  |
  o  A
  

More complete test for status changes (issue5732)
-------------------------------------------------

Generates history of files having 3 states, r0_r1_wc:

 r0: ground (content/missing)
 r1: old state to be amended (content/missing, where missing means removed)
 wc: changes to be included in r1 (content/missing-tracked/untracked)

  $ hg init $TESTTMP/wcstates
  $ cd $TESTTMP/wcstates

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 1
  $ hg addremove -q --similarity 0
  $ hg commit -m0

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 2
  $ hg addremove -q --similarity 0
  $ hg commit -m1

  $ $PYTHON $TESTDIR/generateworkingcopystates.py state 2 wc
  $ hg addremove -q --similarity 0
  $ hg forget *_*_*-untracked
  $ rm *_*_missing-*

amend r1 to include wc changes

  $ hg amend
  saved backup bundle to * (glob) (obsstore-off !)

clean/modified/removed/added states of the amended revision

  $ hg status --all --change . 'glob:content1_*_content1-tracked'
  C content1_content1_content1-tracked
  C content1_content2_content1-tracked
  C content1_missing_content1-tracked
  $ hg status --all --change . 'glob:content1_*_content[23]-tracked'
  M content1_content1_content3-tracked
  M content1_content2_content2-tracked
  M content1_content2_content3-tracked
  M content1_missing_content3-tracked
  $ hg status --all --change . 'glob:content1_*_missing-tracked'
  M content1_content2_missing-tracked
  R content1_missing_missing-tracked
  C content1_content1_missing-tracked
  $ hg status --all --change . 'glob:content1_*_*-untracked'
  R content1_content1_content1-untracked
  R content1_content1_content3-untracked
  R content1_content1_missing-untracked
  R content1_content2_content1-untracked
  R content1_content2_content2-untracked
  R content1_content2_content3-untracked
  R content1_content2_missing-untracked
  R content1_missing_content1-untracked
  R content1_missing_content3-untracked
  R content1_missing_missing-untracked
  $ hg status --all --change . 'glob:missing_content2_*'
  A missing_content2_content2-tracked
  A missing_content2_content3-tracked
  A missing_content2_missing-tracked
  $ hg status --all --change . 'glob:missing_missing_*'
  A missing_missing_content3-tracked

working directory should be all clean (with some missing/untracked files)

  $ hg status --all 'glob:*_content?-tracked'
  C content1_content1_content1-tracked
  C content1_content1_content3-tracked
  C content1_content2_content1-tracked
  C content1_content2_content2-tracked
  C content1_content2_content3-tracked
  C content1_missing_content1-tracked
  C content1_missing_content3-tracked
  C missing_content2_content2-tracked
  C missing_content2_content3-tracked
  C missing_missing_content3-tracked
  $ hg status --all 'glob:*_missing-tracked'
  ! content1_content1_missing-tracked
  ! content1_content2_missing-tracked
  ! content1_missing_missing-tracked
  ! missing_content2_missing-tracked
  ! missing_missing_missing-tracked
  $ hg status --all 'glob:*-untracked'
  ? content1_content1_content1-untracked
  ? content1_content1_content3-untracked
  ? content1_content2_content1-untracked
  ? content1_content2_content2-untracked
  ? content1_content2_content3-untracked
  ? content1_missing_content1-untracked
  ? content1_missing_content3-untracked
  ? missing_content2_content2-untracked
  ? missing_content2_content3-untracked
  ? missing_missing_content3-untracked
