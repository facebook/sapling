
Test histedit extension: Fold commands
======================================

This test file is dedicated to testing the fold command in non conflicting
case.

Initialization
---------------

  $ eagerepo
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable histedit
  $ readconfig <<EOF
  > [alias]
  > logt = log --template '{node|short} {desc|firstline}\n'
  > EOF

  $ enable morestatus
  $ setconfig morestatus.show=true


Simple folding
--------------------
  $ addwithdate ()
  > {
  >     echo $1 > $1
  >     sl add $1
  >     sl ci -m $1 -d "$2 0"
  > }

  $ initrepo ()
  > {
  >     sl init r
  >     cd r
  >     addwithdate a 1
  >     addwithdate b 2
  >     addwithdate c 3
  >     addwithdate d 4
  >     addwithdate e 5
  >     addwithdate f 6
  > }

  $ initrepo

log before edit
  $ sl logt --graph
  @  178e35e0ce73 f
  │
  o  1ddb6c90f2ee e
  │
  o  532247a8969b d
  │
  o  ff2c9fa2018b c
  │
  o  97d72e5f12c7 b
  │
  o  8580ff50825a a
  

  $ sl histedit ff2c9fa2018b --commands - <<EOF | fixbundle
  > pick 1ddb6c90f2ee e
  > pick 178e35e0ce73 f
  > fold ff2c9fa2018b c
  > pick 532247a8969b d
  > EOF

log after edit
  $ sl logt --graph
  @  54f64c576eaf d
  │
  o  7c6777b45203 f
  │
  o  dcc6f3975330 e
  │
  o  97d72e5f12c7 b
  │
  o  8580ff50825a a
  

post-fold manifest
  $ sl manifest
  a
  b
  c
  d
  e
  f


check histedit_source, including that it uses the later date, from the first changeset

  $ sl log --debug --rev 'max(desc(f))'
  commit:      7c6777b45203a557f268e11d9c25fa3038f1d4a9
  phase:       draft
  manifest:    81eede616954057198ead0b2c73b41d1f392829a
  user:        test
  date:        Thu Jan 01 00:00:03 1970 +0000
  files+:      c f
  extra:       branch=default
  extra:       histedit_source=111f74b802e1dc2fb4aa83f846674862f75e930e,ff2c9fa2018b15fa74b33363bda9527323e2a99f
  description:
  f
  ***
  c
  
  

rollup will fold without preserving the folded commit's message or date

  $ OLDHGEDITOR=$HGEDITOR
  $ HGEDITOR=false
  $ sl histedit 97d72e5f12c7 --commands - <<EOF | fixbundle
  > pick 97d72e5f12c7 b
  > roll dcc6f3975330 e
  > pick 7c6777b45203 f
  > pick 54f64c576eaf d
  > EOF

  $ HGEDITOR=$OLDHGEDITOR

log after edit
  $ sl logt --graph
  @  d965c106234a d
  │
  o  8a359a5848bb f
  │
  o  a2e8b40131dd b
  │
  o  8580ff50825a a
  

description is taken from rollup target commit

  $ sl log --debug --rev 'max(desc(b))'
  commit:      a2e8b40131dd3d8a86b4fb1d62a449187afc12c1
  phase:       draft
  manifest:    b5e112a3a8354e269b1524729f0918662d847c38
  user:        test
  date:        Thu Jan 01 00:00:02 1970 +0000
  files+:      b e
  extra:       branch=default
  extra:       histedit_source=97d72e5f12c7e84f85064aa72e5a297142c36ed9,dcc6f3975330bf69a702e82d61e649698e6c9b7a
  description:
  b
  
  

check saving last-message.txt

  $ cat > $TESTTMP/abortfolding.py <<EOF
  > from binascii import unhexlify as bin
  > def abortfolding(repo, node, **kwargs):
  >     commits = repo.commits()
  >     assert node is not None
  >     fields = commits.getcommitfields(bin(node))
  >     if set(fields.files()) == {'c', 'd', 'f'}:
  >         return 1  # return non-zero to abort folding commit only
  > EOF
  $ cat > .sl/config <<EOF
  > [hooks]
  > pretxncommit.abortfolding = python:$TESTTMP/abortfolding.py:abortfolding
  > EOF

  $ cat > $TESTTMP/editor.sh << EOF
  > echo "==== before editing"
  > cat \$1
  > echo "===="
  > echo "check saving last-message.txt" >> \$1
  > EOF

  $ rm -f .sl/last-message.txt
  $ sl status --rev '8a359a5848bb^1::d965c106234a'
  A c
  A d
  A f
  $ HGEDITOR="sh $TESTTMP/editor.sh" sl histedit 8a359a5848bb --commands - <<EOF
  > pick 8a359a5848bb f
  > fold d965c106234a d
  > EOF
  ==== before editing
  f
  ***
  c
  ***
  d
  
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: added c
  SL: added d
  SL: added f
  ====
  abort: pretxncommit.abortfolding hook failed
  [255]

  $ cat .sl/last-message.txt
  f
  ***
  c
  ***
  d
  
  
  
  check saving last-message.txt

  $ cd ..
  $ rm -r r

folding preserves initial author but uses later date
----------------------------------------------------

  $ initrepo

  $ sl ci -d '7 0' --user "someone else" --amend --quiet

tip before edit
  $ sl log --rev .
  commit:      10c36dd37515
  user:        someone else
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     f
  

  $ sl --config progress.debug=1 --debug \
  > histedit 1ddb6c90f2ee --commands - 2>&1 <<EOF | \
  > grep -E 'editing|unresolved'
  > pick 1ddb6c90f2ee e
  > fold 10c36dd37515 f
  > EOF
  progress: editing: pick 1ddb6c90f2ee e 1/2 changes (50.00%)
  progress: editing: fold 10c36dd37515 f 2/2 changes (100.00%)
  progress: editing (end)

tip after edit, which should use the later date, from the second changeset
  $ sl log --rev .
  commit:      e4f3ec5d0b40
  user:        test
  date:        Thu Jan 01 00:00:07 1970 +0000
  summary:     e
  

  $ cd ..
  $ rm -r r

folding and creating no new change doesn't break:
-------------------------------------------------

folded content is dropped during a merge. The folded commit should properly disappear.

  $ mkdir fold-to-empty-test
  $ cd fold-to-empty-test
  $ sl init
  $ printf "1\n2\n3\n" > file
  $ sl add file
  $ sl commit -m '1+2+3'
  $ echo 4 >> file
  $ sl commit -m '+4'
  $ echo 5 >> file
  $ sl commit -m '+5'
  $ echo 6 >> file
  $ sl commit -m '+6'
  $ sl logt --graph
  @  251d831eeec5 +6
  │
  o  888f9082bf99 +5
  │
  o  617f94f13c0f +4
  │
  o  0189ba417d34 1+2+3
  

  $ sl histedit 617f94f13c0faff2ff307641901637b91cbd7c7b --commands - << EOF
  > pick 617f94f13c0f 1 +4
  > drop 888f9082bf99 2 +5
  > fold 251d831eeec5 3 +6
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'sl resolve --mark')
  Fix up the change (fold 251d831eeec5)
  (sl histedit --continue to resume)
  [1]
There were conflicts, we keep P1 content. This
should effectively drop the changes from +6.

  $ sl status
  M file
  ? file.orig
  
  # The repository is in an unfinished *histedit* state.
  # Unresolved merge conflicts (1):
  # 
  #     file
  # 
  # To mark files as resolved:  sl resolve --mark FILE
  # To continue:                sl histedit --continue
  # To abort:                   sl histedit --abort

  $ sl resolve -l
  U file
  $ sl revert -r 'p1()' file
  $ sl resolve --mark file
  (no more unresolved files)
  continue: sl histedit --continue
  $ sl histedit --continue
  251d831eeec5: empty changeset
  $ sl logt --graph
  @  617f94f13c0f +4
  │
  o  0189ba417d34 1+2+3
  

  $ cd ..


Test fold through dropped
-------------------------


Test corner case where folded revision is separated from its parent by a
dropped revision.


  $ sl init fold-with-dropped
  $ cd fold-with-dropped
  $ printf "1\n2\n3\n" > file
  $ sl commit -Am '1+2+3'
  adding file
  $ echo 4 >> file
  $ sl commit -m '+4'
  $ echo 5 >> file
  $ sl commit -m '+5'
  $ echo 6 >> file
  $ sl commit -m '+6'
  $ sl logt -G
  @  251d831eeec5 +6
  │
  o  888f9082bf99 +5
  │
  o  617f94f13c0f +4
  │
  o  0189ba417d34 1+2+3
  
  $ sl histedit 617f94f13c0faff2ff307641901637b91cbd7c7b --commands -  << EOF
  > pick 617f94f13c0f 1 +4
  > drop 888f9082bf99 2 +5
  > fold 251d831eeec5 3 +6
  > EOF
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging file
  warning: 1 conflicts while merging file! (edit, then use 'sl resolve --mark')
  Fix up the change (fold 251d831eeec5)
  (sl histedit --continue to resume)
  [1]
  $ cat > file << EOF
  > 1
  > 2
  > 3
  > 4
  > 5
  > EOF
  $ sl resolve --mark file
  (no more unresolved files)
  continue: sl histedit --continue
  $ sl commit -m '+5.2'
  $ echo 6 >> file
  $ HGEDITOR=cat sl histedit --continue
  +4
  ***
  +5.2
  ***
  +6
  
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: changed file
  $ sl logt -G
  @  10c647b2cdd5 +4
  │
  o  0189ba417d34 1+2+3
  
  $ sl export tip
  # SL changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 10c647b2cdd54db0603ecb99b2ff5ce66d5a5323
  # Parent  0189ba417d34df9dda55f88b637dcae9917b5964
  +4
  ***
  +5.2
  ***
  +6
  
  diff -r 0189ba417d34 -r 10c647b2cdd5 file
  --- a/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,6 @@
   1
   2
   3
  +4
  +5
  +6
  $ cd ..


Folding with initial rename (issue3729)
---------------------------------------

  $ sl init fold-rename
  $ cd fold-rename
  $ echo a > a.txt
  $ sl add a.txt
  $ sl commit -m a
  $ sl rename a.txt b.txt
  $ sl commit -m rename
  $ echo b >> b.txt
  $ sl commit -m b

  $ sl logt --follow b.txt
  e0371e0426bc b
  1c4f440a8085 rename
  6c795aa153cb a

  $ sl histedit 1c4f440a8085 --commands - << EOF | fixbundle
  > pick 1c4f440a8085 rename
  > fold e0371e0426bc b
  > EOF

  $ sl logt --follow b.txt
  cf858d235c76 rename
  6c795aa153cb a

  $ cd ..

Folding with swapping
---------------------

This is an excuse to test hook with histedit temporary commit (issue4422)


  $ sl init issue4422
  $ cd issue4422
  $ echo a > a.txt
  $ sl add a.txt
  $ sl commit -m a
  $ echo b > b.txt
  $ sl add b.txt
  $ sl commit -m b
  $ echo c > c.txt
  $ sl add c.txt
  $ sl commit -m c

  $ sl logt
  a1a953ffb4b0 c
  199b6bb90248 b
  6c795aa153cb a

Setup the proper environment variable symbol for the platform, to be subbed
into the hook command.
#if windows
  $ NODE="%HG_NODE%"
#else
  $ NODE="\$HG_NODE"
#endif
  $ sl histedit 6c795aa153cb --config hooks.commit="echo commit $NODE" --commands - << EOF | fixbundle
  > pick 199b6bb90248 b
  > fold a1a953ffb4b0 c
  > pick 6c795aa153cb a
  > EOF
  commit 16b87e97178dde2af2f3c6f6ddda882292f21d13
  commit 9599899f62c05f4377548c32bf1c9f1a39634b0c

  $ sl logt
  9599899f62c0 a
  79b99e9c8e49 b

  $ echo "foo" > amended.txt
  $ sl add amended.txt
  $ sl ci -q --amend -I amended.txt

Test that folding multiple changes in a row doesn't show multiple
editors.

  $ echo foo >> foo
  $ sl add foo
  $ sl ci -m foo1
  $ echo foo >> foo
  $ sl ci -m foo2
  $ echo foo >> foo
  $ sl ci -m foo3
  $ sl logt
  21679ff7675c foo3
  b7389cc4d66e foo2
  0e01aeef5fa8 foo1
  578c7455730c a
  79b99e9c8e49 b
  $ cat > "$TESTTMP/editor.sh" <<EOF
  > echo ran editor >> "$TESTTMP/editorlog.txt"
  > cat \$1 >> "$TESTTMP/editorlog.txt"
  > echo END >> "$TESTTMP/editorlog.txt"
  > echo merged foos > \$1
  > EOF
  $ HGEDITOR="sh \"$TESTTMP/editor.sh\"" sl histedit 'max(desc(a))' --commands - <<EOF | fixbundle
  > pick 578c7455730c 1 a
  > pick 0e01aeef5fa8 2 foo1
  > fold b7389cc4d66e 3 foo2
  > fold 21679ff7675c 4 foo3
  > EOF
  $ sl logt
  e8bedbda72c1 merged foos
  578c7455730c a
  79b99e9c8e49 b
Editor should have run only once
  $ cat $TESTTMP/editorlog.txt
  ran editor
  foo1
  ***
  foo2
  ***
  foo3
  
  
  
  SL: Enter commit message.  Lines beginning with 'SL:' are removed.
  SL: Leave message empty to abort commit.
  SL: --
  SL: user: test
  SL: added foo
  END

  $ cd ..

Test rolling into a commit with multiple children (issue5498)

  $ sl init roll
  $ cd roll
  $ echo a > a
  $ sl commit -qAm aa
  $ echo b > b
  $ sl commit -qAm bb
  $ sl up -q ".^"
  $ echo c > c
  $ sl commit -qAm cc
  $ sl log -G -T '{node|short} {desc}'
  @  5db65b93a12b cc
  │
  │ o  301d76bdc3ae bb
  ├─╯
  o  8f0162e483d0 aa
  

  $ sl histedit . --commands - << EOF
  > r 5db65b93a12b
  > EOF
  sl: parse error: first changeset cannot use verb "roll"
  [255]
  $ sl log -G -T '{node|short} {desc}'
  @  5db65b93a12b cc
  │
  │ o  301d76bdc3ae bb
  ├─╯
  o  8f0162e483d0 aa
  

  $ cd ..

Fold/roll shouldn't trigger a merge:

  $ sl init rollmerge
  $ cd rollmerge
  $ echo a > a
  $ sl commit -qAm a
  $ echo b > a
  $ sl commit -qAm b
  $ sl log -G -T '{node|short} {desc}'
  @  1e6c11564562 b
  │
  o  cb9a9f314b8b a
  
Set a bogus mergedriver as a tripwire to make sure we don't invoke merge driver.
  $ sl histedit --config extensions.mergedriver= --config experimental.mergedriver=dontrunthis --commands - << EOF
  > p cb9a9f314b8b
  > r 1e6c11564562
  > EOF
  $ sl log -G -T '{node|short} {desc}'
  @  9e233947f73d a
  

  $ cd

  $ sl init folddelete
  $ cd folddelete
  $ drawdag <<EOS
  > D  # D/file2 = foo\n
  > |  # D/file1 = (removed)
  > |
  > C  # C/file1 = foo\n
  > |  # C/file2 = (removed)
  > |
  > B  # B/file2 = foo\n
  > |  # B/file1 = (removed)
  > |  # B/create = create\n
  > |
  > A  # A/file1 = foo\n
  >    # drawdag.defaultfiles=false
  > EOS
  $ sl go -q $D
  $ sl histedit $B --commands - <<EOF
  > pick $B
  > fold $C
  > pick $D
  > EOF
  $ sl log -G -p --config diff.git=1
  @  commit:      048204e0ad0b
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     D
  │
  │  diff --git a/file1 b/file1
  │  deleted file mode 100644
  │  --- a/file1
  │  +++ /dev/null
  │  @@ -1,1 +0,0 @@
  │  -foo
  │  diff --git a/file2 b/file2
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/file2
  │  @@ -0,0 +1,1 @@
  │  +foo
  │
  o  commit:      8f8b4b8421e6
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  │  diff --git a/create b/create
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/create
  │  @@ -0,0 +1,1 @@
  │  +create
  │
  o  commit:      e1d505319e55
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
     diff --git a/file1 b/file1
     new file mode 100644
     --- /dev/null
     +++ b/file1
     @@ -0,0 +1,1 @@
     +foo
  
  $ sl st
  $ ls
  create
  file2
  $ sl files -r .
  create
  file2
