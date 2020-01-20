#chg-compatible

  $ disable treemanifest
#require git

  $ echo "[core]" >> $HOME/.gitconfig
  $ echo "autocrlf = false" >> $HOME/.gitconfig
  $ echo "[core]" >> $HOME/.gitconfig
  $ echo "autocrlf = false" >> $HOME/.gitconfig
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ cat >> $HGRCPATH <<EOF
  > [subrepos]
  > git:allowed = true
  > EOF
  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE
  $ INVALIDID1=afd12345af
  $ INVALIDID2=28173x36ddd1e67bf7098d541130558ef5534a86
  $ VALIDID1=39b3d83f9a69a9ba4ebb111461071a0af0027357
  $ VALIDID2=8dd6476bd09d9c7776355dc454dafe38efaec5da
  $ count=10
  $ commit()
  > {
  >     GIT_AUTHOR_DATE="2007-01-01 00:00:$count +0000"
  >     GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"
  >     git commit "$@" >/dev/null 2>/dev/null || echo "git commit error"
  >     count=`expr $count + 1`
  > }
  $ mkdir git-repo
  $ cd git-repo
  $ git init-db >/dev/null 2>/dev/null
  $ echo a > a
  $ mkdir d
  $ echo b > d/b
  $ git add a d
  $ commit -a -m t1

Remove the directory, then try to replace it with a file (issue754)

  $ git rm -f d/b
  rm 'd/b'
  $ commit -m t2
  $ echo d > d
  $ git add d
  $ commit -m t3
  $ echo b >> a
  $ commit -a -m t4.1
  $ git checkout -b other HEAD~ >/dev/null 2>/dev/null
  $ echo c > a
  $ echo a >> a
  $ commit -a -m t4.2
  $ git checkout master >/dev/null 2>/dev/null
  $ git pull --no-commit . other > /dev/null 2>/dev/null
  $ commit -m 'Merge branch other'
  $ cd ..
  $ hg convert --config extensions.progress= --config progress.debug=1 \
  >            --datesort git-repo
  assuming destination git-repo-hg
  initializing destination git-repo-hg repository
  scanning source...
  progress: scanning: 1/6 revisions (16.67%)
  progress: scanning: 2/6 revisions (33.33%)
  progress: scanning: 3/6 revisions (50.00%)
  progress: scanning: 4/6 revisions (66.67%)
  progress: scanning: 5/6 revisions (83.33%)
  progress: scanning: 6/6 revisions (100.00%)
  progress: scanning (end)
  sorting...
  converting...
  5 t1
  progress: converting: 0/6 revisions (0.00%)
  4 t2
  progress: converting: 1/6 revisions (16.67%)
  3 t3
  progress: converting: 2/6 revisions (33.33%)
  2 t4.1
  progress: converting: 3/6 revisions (50.00%)
  1 t4.2
  progress: converting: 4/6 revisions (66.67%)
  0 Merge branch other
  progress: converting: 5/6 revisions (83.33%)
  progress: converting (end)
  updating bookmarks
  $ hg up -q -R git-repo-hg
  $ hg -R git-repo-hg tip -v
  changeset:   5:c78094926be2
  bookmark:    master
  parent:      3:f5f5cb45432b
  parent:      4:4e174f80c67c
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:15 2007 +0000
  files:       a
  description:
  Merge branch other
  
  
  $ count=10
  $ mkdir git-repo2
  $ cd git-repo2
  $ git init-db >/dev/null 2>/dev/null
  $ echo foo > foo
  $ git add foo
  $ commit -a -m 'add foo'
  $ echo >> foo
  $ commit -a -m 'change foo'
  $ git checkout -b Bar HEAD~ >/dev/null 2>/dev/null
  $ echo quux >> quux
  $ git add quux
  $ commit -a -m 'add quux'
  $ echo bar > bar
  $ git add bar
  $ commit -a -m 'add bar'
  $ git checkout -b Baz HEAD~ >/dev/null 2>/dev/null
  $ echo baz > baz
  $ git add baz
  $ commit -a -m 'add baz'
  $ git checkout master >/dev/null 2>/dev/null
  $ git pull --no-commit . Bar Baz > /dev/null 2>/dev/null
  $ commit -m 'Octopus merge'
  $ echo bar >> bar
  $ commit -a -m 'change bar'
  $ git checkout -b Foo HEAD~ >/dev/null 2>/dev/null
  $ echo >> foo
  $ commit -a -m 'change foo'
  $ git checkout master >/dev/null 2>/dev/null
  $ git pull --no-commit -s ours . Foo > /dev/null 2>/dev/null
  $ commit -m 'Discard change to foo'
  $ cd ..
  $ glog()
  > {
  >     hg log -G --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
  > }
  $ splitrepo()
  > {
  >     msg="$1"
  >     files="$2"
  >     opts=$3
  >     echo "% $files: $msg"
  >     prefix=`echo "$files" | sed -e 's/ /-/g'`
  >     fmap="$prefix.fmap"
  >     repo="$prefix.repo"
  >     for i in $files; do
  >         echo "include $i" >> "$fmap"
  >     done
  >     hg -q convert $opts --filemap "$fmap" --datesort git-repo2 "$repo"
  >     hg up -q -R "$repo"
  >     glog -R "$repo"
  >     hg -R "$repo" manifest --debug
  > }

full conversion

  $ hg convert --datesort git-repo2 fullrepo \
  > --config extensions.progress= --config progress.debug=1
  initializing destination fullrepo repository
  scanning source...
  progress: scanning: 1/9 revisions (11.11%)
  progress: scanning: 2/9 revisions (22.22%)
  progress: scanning: 3/9 revisions (33.33%)
  progress: scanning: 4/9 revisions (44.44%)
  progress: scanning: 5/9 revisions (55.56%)
  progress: scanning: 6/9 revisions (66.67%)
  progress: scanning: 7/9 revisions (77.78%)
  progress: scanning: 8/9 revisions (88.89%)
  progress: scanning: 9/9 revisions (100.00%)
  progress: scanning (end)
  sorting...
  converting...
  8 add foo
  progress: converting: 0/9 revisions (0.00%)
  7 change foo
  progress: converting: 1/9 revisions (11.11%)
  6 add quux
  progress: converting: 2/9 revisions (22.22%)
  5 add bar
  progress: converting: 3/9 revisions (33.33%)
  4 add baz
  progress: converting: 4/9 revisions (44.44%)
  3 Octopus merge
  progress: converting: 5/9 revisions (55.56%)
  2 change bar
  progress: converting: 6/9 revisions (66.67%)
  1 change foo
  progress: converting: 7/9 revisions (77.78%)
  0 Discard change to foo
  progress: converting: 8/9 revisions (88.89%)
  progress: converting (end)
  updating bookmarks
  $ hg up -q -R fullrepo
  $ glog -R fullrepo
  @    9 "Discard change to foo" files: foo
  |\
  | o  8 "change foo" files: foo
  | |
  o |  7 "change bar" files: bar
  |/
  o    6 "(octopus merge fixup)" files:
  |\
  | o    5 "Octopus merge" files: baz
  | |\
  o | |  4 "add baz" files: baz
  | | |
  +---o  3 "add bar" files: bar
  | |
  o |  2 "add quux" files: quux
  | |
  | o  1 "change foo" files: foo
  |/
  o  0 "add foo" files: foo
  
  $ hg -R fullrepo manifest --debug
  245a3b8bc653999c2b22cdabd517ccb47aecafdf 644   bar
  354ae8da6e890359ef49ade27b68bbc361f3ca88 644   baz
  9277c9cc8dd4576fc01a17939b4351e5ada93466 644   foo
  88dfeab657e8cf2cef3dec67b914f49791ae76b1 644   quux
  $ splitrepo 'octopus merge' 'foo bar baz'
  % foo bar baz: octopus merge
  @    8 "Discard change to foo" files: foo
  |\
  | o  7 "change foo" files: foo
  | |
  o |  6 "change bar" files: bar
  |/
  o    5 "(octopus merge fixup)" files:
  |\
  | o    4 "Octopus merge" files: baz
  | |\
  o | |  3 "add baz" files: baz
  | | |
  +---o  2 "add bar" files: bar
  | |
  | o  1 "change foo" files: foo
  |/
  o  0 "add foo" files: foo
  
  245a3b8bc653999c2b22cdabd517ccb47aecafdf 644   bar
  354ae8da6e890359ef49ade27b68bbc361f3ca88 644   baz
  9277c9cc8dd4576fc01a17939b4351e5ada93466 644   foo
  $ splitrepo 'only some parents of an octopus merge; "discard" a head' 'foo baz quux'
  % foo baz quux: only some parents of an octopus merge; "discard" a head
  @  6 "Discard change to foo" files: foo
  |
  o  5 "change foo" files: foo
  |
  o    4 "Octopus merge" files:
  |\
  | o  3 "add baz" files: baz
  | |
  | o  2 "add quux" files: quux
  | |
  o |  1 "change foo" files: foo
  |/
  o  0 "add foo" files: foo
  
  354ae8da6e890359ef49ade27b68bbc361f3ca88 644   baz
  9277c9cc8dd4576fc01a17939b4351e5ada93466 644   foo
  88dfeab657e8cf2cef3dec67b914f49791ae76b1 644   quux

test importing git renames and copies

  $ cd git-repo2
  $ git mv foo foo-renamed
since bar is not touched in this commit, this copy will not be detected
  $ cp bar bar-copied
  $ cp baz baz-copied
  $ cp baz baz-copied2
  $ cp baz ba-copy
  $ echo baz2 >> baz
  $ git add bar-copied baz-copied baz-copied2 ba-copy
  $ commit -a -m 'rename and copy'
  $ cd ..

input validation
  $ hg convert --config convert.git.similarity=foo --datesort git-repo2 fullrepo
  abort: convert.git.similarity is not a valid integer ('foo')
  [255]
  $ hg convert --config convert.git.similarity=-1 --datesort git-repo2 fullrepo
  abort: similarity must be between 0 and 100
  [255]
  $ hg convert --config convert.git.similarity=101 --datesort git-repo2 fullrepo
  abort: similarity must be between 0 and 100
  [255]

  $ hg -q convert --config convert.git.similarity=100 --datesort git-repo2 fullrepo
  $ hg -R fullrepo status -C --change master
  M baz
  A ba-copy
    baz
  A bar-copied
  A baz-copied
    baz
  A baz-copied2
    baz
  A foo-renamed
    foo
  R foo

Ensure that the modification to the copy source was preserved
(there was a bug where if the copy dest was alphabetically prior to the copy
source, the copy source took the contents of the copy dest)
  $ hg cat -r tip fullrepo/baz
  baz
  baz2

  $ cd git-repo2
  $ echo bar2 >> bar
  $ commit -a -m 'change bar'
  $ cp bar bar-copied2
  $ git add bar-copied2
  $ commit -a -m 'copy with no changes'
  $ cd ..

  $ hg -q convert --config convert.git.similarity=100 \
  > --config convert.git.findcopiesharder=1 --datesort git-repo2 fullrepo
  $ hg -R fullrepo status -C --change master
  A bar-copied2
    bar

renamelimit config option works

  $ cd git-repo2
  $ cat >> copy-source << EOF
  > sc0
  > sc1
  > sc2
  > sc3
  > sc4
  > sc5
  > sc6
  > EOF
  $ git add copy-source
  $ commit -m 'add copy-source'
  $ cp copy-source source-copy0
  $ echo 0 >> source-copy0
  $ cp copy-source source-copy1
  $ echo 1 >> source-copy1
  $ git add source-copy0 source-copy1
  $ commit -a -m 'copy copy-source 2 times'
  $ cd ..

  $ hg -q convert --config convert.git.renamelimit=1 \
  > --config convert.git.findcopiesharder=true --datesort git-repo2 fullrepo2
  $ hg -R fullrepo2 status -C --change master
  A source-copy0
  A source-copy1

  $ hg -q convert --config convert.git.renamelimit=100 \
  > --config convert.git.findcopiesharder=true --datesort git-repo2 fullrepo3
  $ hg -R fullrepo3 status -C --change master
  A source-copy0
    copy-source
  A source-copy1
    copy-source

test binary conversion (issue1359)

  $ count=19
  $ mkdir git-repo3
  $ cd git-repo3
  $ git init-db >/dev/null 2>/dev/null
  $ $PYTHON -c 'file("b", "wb").write("".join([chr(i) for i in range(256)])*16)'
  $ git add b
  $ commit -a -m addbinary
  $ cd ..

convert binary file

  $ hg convert git-repo3 git-repo3-hg
  initializing destination git-repo3-hg repository
  scanning source...
  sorting...
  converting...
  0 addbinary
  updating bookmarks
  $ cd git-repo3-hg
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ $PYTHON -c 'print len(file("b", "rb").read())'
  4096
  $ cd ..

test author vs committer

  $ mkdir git-repo4
  $ cd git-repo4
  $ git init-db >/dev/null 2>/dev/null
  $ echo >> foo
  $ git add foo
  $ commit -a -m addfoo
  $ echo >> foo
  $ GIT_AUTHOR_NAME="nottest"
  $ commit -a -m addfoo2
  $ cd ..

convert author committer

  $ hg convert git-repo4 git-repo4-hg
  initializing destination git-repo4-hg repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks
  $ hg -R git-repo4-hg log -v
  changeset:   1:d63e967f93da
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  committer: test <test@example.org>
  
  
  changeset:   0:0735477b0224
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  

Various combinations of committeractions fail

  $ hg --config convert.git.committeractions=messagedifferent,messagealways convert git-repo4 bad-committer
  initializing destination bad-committer repository
  abort: committeractions cannot define both messagedifferent and messagealways
  [255]

  $ hg --config convert.git.committeractions=dropcommitter,replaceauthor convert git-repo4 bad-committer
  initializing destination bad-committer repository
  abort: committeractions cannot define both dropcommitter and replaceauthor
  [255]

  $ hg --config convert.git.committeractions=dropcommitter,messagealways convert git-repo4 bad-committer
  initializing destination bad-committer repository
  abort: committeractions cannot define both dropcommitter and messagealways
  [255]

custom prefix on messagedifferent works

  $ hg --config convert.git.committeractions=messagedifferent=different: convert git-repo4 git-repo4-hg-messagedifferentprefix
  initializing destination git-repo4-hg-messagedifferentprefix repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks

  $ hg -R git-repo4-hg-messagedifferentprefix log -v
  changeset:   1:2fe0c98a109d
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  different: test <test@example.org>
  
  
  changeset:   0:0735477b0224
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  

messagealways will always add the "committer: " line even if committer identical

  $ hg --config convert.git.committeractions=messagealways convert git-repo4 git-repo4-hg-messagealways
  initializing destination git-repo4-hg-messagealways repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks

  $ hg -R git-repo4-hg-messagealways log -v
  changeset:   1:8db057d8cd37
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  committer: test <test@example.org>
  
  
  changeset:   0:8f71fe9c98be
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  committer: test <test@example.org>
  
  

custom prefix on messagealways works

  $ hg --config convert.git.committeractions=messagealways=always: convert git-repo4 git-repo4-hg-messagealwaysprefix
  initializing destination git-repo4-hg-messagealwaysprefix repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks

  $ hg -R git-repo4-hg-messagealwaysprefix log -v
  changeset:   1:83c17174de79
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  always: test <test@example.org>
  
  
  changeset:   0:2ac9bcb3534a
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  always: test <test@example.org>
  
  

replaceauthor replaces author with committer

  $ hg --config convert.git.committeractions=replaceauthor convert git-repo4 git-repo4-hg-replaceauthor
  initializing destination git-repo4-hg-replaceauthor repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks

  $ hg -R git-repo4-hg-replaceauthor log -v
  changeset:   1:122c1d8999ea
  bookmark:    master
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  
  changeset:   0:0735477b0224
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  

dropcommitter removes the committer

  $ hg --config convert.git.committeractions=dropcommitter convert git-repo4 git-repo4-hg-dropcommitter
  initializing destination git-repo4-hg-dropcommitter repository
  scanning source...
  sorting...
  converting...
  1 addfoo
  0 addfoo2
  updating bookmarks

  $ hg -R git-repo4-hg-dropcommitter log -v
  changeset:   1:190b2da396cc
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:21 2007 +0000
  files:       foo
  description:
  addfoo2
  
  
  changeset:   0:0735477b0224
  user:        test <test@example.org>
  date:        Mon Jan 01 00:00:20 2007 +0000
  files:       foo
  description:
  addfoo
  
  

--sourceorder should fail

  $ hg convert --sourcesort git-repo4 git-repo4-sourcesort-hg
  initializing destination git-repo4-sourcesort-hg repository
  abort: --sourcesort is not supported by this data source
  [255]

test converting certain branches

  $ mkdir git-testrevs
  $ cd git-testrevs
  $ git init
  Initialized empty Git repository in $TESTTMP/git-testrevs/.git/
  $ echo a >> a ; git add a > /dev/null; git commit -m 'first' > /dev/null
  $ echo a >> a ; git add a > /dev/null; git commit -m 'master commit' > /dev/null
  $ git checkout -b goodbranch 'HEAD^'
  Switched to a new branch 'goodbranch'
  $ echo a >> b ; git add b > /dev/null; git commit -m 'good branch commit' > /dev/null
  $ git checkout -b badbranch 'HEAD^'
  Switched to a new branch 'badbranch'
  $ echo a >> c ; git add c > /dev/null; git commit -m 'bad branch commit' > /dev/null
  $ cd ..
  $ hg convert git-testrevs hg-testrevs --rev master --rev goodbranch
  initializing destination hg-testrevs repository
  scanning source...
  sorting...
  converting...
  2 first
  1 good branch commit
  0 master commit
  updating bookmarks
  $ cd hg-testrevs
  $ hg log -G -T '{rev} {bookmarks}'
  o  2 master
  |
  | o  1 goodbranch
  |/
  o  0
  
  $ cd ..

test sub modules

  $ mkdir git-repo5
  $ cd git-repo5
  $ git init-db >/dev/null 2>/dev/null
  $ echo 'sub' >> foo
  $ git add foo
  $ commit -a -m 'addfoo'
  $ BASE=`pwd`
  $ cd ..
  $ mkdir git-repo6
  $ cd git-repo6
  $ git init-db >/dev/null 2>/dev/null
  $ git submodule add ${BASE} >/dev/null 2>/dev/null
  $ commit -a -m 'addsubmodule' >/dev/null 2>/dev/null

test non-tab whitespace .gitmodules

  $ cat >> .gitmodules <<EOF
  > [submodule "git-repo5"]
  >   path = git-repo5
  >   url = git-repo5
  > EOF
  $ git commit -q -a -m "weird white space submodule"
  $ cd ..
  $ hg convert git-repo6 hg-repo6
  initializing destination hg-repo6 repository
  scanning source...
  sorting...
  converting...
  1 addsubmodule
  0 weird white space submodule
  updating bookmarks

  $ rm -rf hg-repo6
  $ cd git-repo6
  $ git reset --hard 'HEAD^' > /dev/null

test missing .gitmodules

  $ git submodule add ../git-repo4 >/dev/null 2>/dev/null
  $ git checkout HEAD .gitmodules
  $ git rm .gitmodules
  rm '.gitmodules'
  $ git commit -q -m "remove .gitmodules" .gitmodules
  $ git commit -q -m "missing .gitmodules"
  $ cd ..
  $ hg convert git-repo6 hg-repo6 --traceback 2>&1 | grep -v "fatal: Path '.gitmodules' does not exist"
  initializing destination hg-repo6 repository
  scanning source...
  sorting...
  converting...
  2 addsubmodule
  1 remove .gitmodules
  0 missing .gitmodules
  updating bookmarks
  $ rm -rf hg-repo6
  $ cd git-repo6
  $ rm -rf git-repo4
  $ git reset --hard 'HEAD^^' > /dev/null
  $ cd ..

test invalid splicemap1

  $ cat > splicemap <<EOF
  > $VALIDID1
  > EOF
  $ hg convert --splicemap splicemap git-repo2 git-repo2-splicemap1-hg
  initializing destination git-repo2-splicemap1-hg repository
  abort: syntax error in splicemap(1): child parent1[,parent2] expected
  [255]

test invalid splicemap2

  $ cat > splicemap <<EOF
  > $VALIDID1 $VALIDID2, $VALIDID2, $VALIDID2
  > EOF
  $ hg convert --splicemap splicemap git-repo2 git-repo2-splicemap2-hg
  initializing destination git-repo2-splicemap2-hg repository
  abort: syntax error in splicemap(1): child parent1[,parent2] expected
  [255]

test invalid splicemap3

  $ cat > splicemap <<EOF
  > $INVALIDID1 $INVALIDID2
  > EOF
  $ hg convert --splicemap splicemap git-repo2 git-repo2-splicemap3-hg
  initializing destination git-repo2-splicemap3-hg repository
  abort: splicemap entry afd12345af is not a valid revision identifier
  [255]

convert sub modules
  $ hg convert git-repo6 git-repo6-hg
  initializing destination git-repo6-hg repository
  scanning source...
  sorting...
  converting...
  0 addsubmodule
  updating bookmarks
  $ hg -R git-repo6-hg log -v
  changeset:   0:* (glob)
  bookmark:    master
  user:        nottest <test@example.org>
  date:        Mon Jan 01 00:00:23 2007 +0000
  description:
  addsubmodule
  
  committer: test <test@example.org>
  
  

  $ cd git-repo6-hg
  $ hg up >/dev/null 2>/dev/null

  $ cd $TESTTMP

make sure rename detection doesn't break removing and adding gitmodules

  $ cd git-repo6
  $ git mv .gitmodules .gitmodules-renamed
  $ commit -a -m 'rename .gitmodules'
  $ git mv .gitmodules-renamed .gitmodules
  $ commit -a -m 'rename .gitmodules back'
  $ cd ..

  $ hg --config convert.git.similarity=100 convert -q git-repo6 git-repo6-hg
  $ hg -R git-repo6-hg log -r 'tip^' -T "{desc|firstline}\n"
  rename .gitmodules
  $ hg -R git-repo6-hg status -C --change 'tip^'
  A .gitmodules-renamed
  $ hg -R git-repo6-hg log -r tip -T "{desc|firstline}\n"
  rename .gitmodules back
  $ hg -R git-repo6-hg status -C --change tip
  R .gitmodules-renamed

convert the revision removing '.gitmodules' itself (and related
submodules)

  $ cd git-repo6
  $ git rm .gitmodules
  rm '.gitmodules'
  $ git rm --cached git-repo5
  rm 'git-repo5'
  $ commit -a -m 'remove .gitmodules and submodule git-repo5'
  $ cd ..

  $ hg convert -q git-repo6 git-repo6-hg
  $ hg -R git-repo6-hg tip -T "{desc|firstline}\n"
  remove .gitmodules and submodule git-repo5
  $ hg -R git-repo6-hg tip -T "{file_dels}\n"
  

skip submodules in the conversion

  $ hg convert -q git-repo6 no-submodules --config convert.git.skipsubmodules=True
  $ hg -R no-submodules manifest --all
  .gitmodules-renamed

convert using a different remote prefix
  $ git init git-repo7
  Initialized empty Git repository in $TESTTMP/git-repo7/.git/
  $ cd git-repo7
TODO: it'd be nice to use (?) lines instead of grep -v to handle the
git output variance, but that doesn't currently work in the middle of
a block, so do this for now.
  $ touch a && git add a && git commit -am "commit a" | grep -v changed
  [master (root-commit) 8ae5f69] commit a
   Author: nottest <test@example.org>
   create mode 100644 a
  $ cd ..
  $ git clone git-repo7 git-repo7-client
  Cloning into 'git-repo7-client'...
  done.
  $ hg convert --config convert.git.remoteprefix=origin git-repo7-client hg-repo7
  initializing destination hg-repo7 repository
  scanning source...
  sorting...
  converting...
  0 commit a
  updating bookmarks
  $ hg -R hg-repo7 bookmarks
     master                    0:03bf38caa4c6
     origin/master             0:03bf38caa4c6

Run convert when the remote branches have changed
(there was an old bug where the local convert read branches from the server)

  $ cd git-repo7
  $ echo a >> a
  $ git commit -q -am "move master forward"
  $ cd ..
  $ rm -rf hg-repo7
  $ hg convert --config convert.git.remoteprefix=origin git-repo7-client hg-repo7
  initializing destination hg-repo7 repository
  scanning source...
  sorting...
  converting...
  0 commit a
  updating bookmarks
  $ hg -R hg-repo7 bookmarks
     master                    0:03bf38caa4c6
     origin/master             0:03bf38caa4c6

damaged git repository tests:
In case the hard-coded hashes change, the following commands can be used to
list the hashes and their corresponding types in the repository:
cd git-repo4/.git/objects
find . -type f | cut -c 3- | sed 's_/__' | xargs -n 1 -t git cat-file -t
cd ../../..

damage git repository by renaming a commit object
  $ COMMIT_OBJ=1c/0ce3c5886f83a1d78a7b517cdff5cf9ca17bdd
  $ mv git-repo4/.git/objects/$COMMIT_OBJ git-repo4/.git/objects/$COMMIT_OBJ.tmp
  $ hg convert git-repo4 git-repo4-broken-hg 2>&1 | grep 'abort:'
  abort: cannot retrieve number of commits in $TESTTMP/git-repo4/.git
  $ mv git-repo4/.git/objects/$COMMIT_OBJ.tmp git-repo4/.git/objects/$COMMIT_OBJ
damage git repository by renaming a blob object

  $ BLOB_OBJ=8b/137891791fe96927ad78e64b0aad7bded08bdc
  $ mv git-repo4/.git/objects/$BLOB_OBJ git-repo4/.git/objects/$BLOB_OBJ.tmp
  $ hg convert git-repo4 git-repo4-broken-hg 2>&1 | grep 'abort:'
  abort: cannot read 'blob' object at 8b137891791fe96927ad78e64b0aad7bded08bdc
  $ mv git-repo4/.git/objects/$BLOB_OBJ.tmp git-repo4/.git/objects/$BLOB_OBJ
damage git repository by renaming a tree object

  $ TREE_OBJ=72/49f083d2a63a41cc737764a86981eb5f3e4635
  $ mv git-repo4/.git/objects/$TREE_OBJ git-repo4/.git/objects/$TREE_OBJ.tmp
  $ hg convert git-repo4 git-repo4-broken-hg 2>&1 | grep 'abort:'
  abort: cannot read changes in 1c0ce3c5886f83a1d78a7b517cdff5cf9ca17bdd

#if no-windows git19

test for escaping the repo name (CVE-2016-3069)

  $ git init '`echo pwned >COMMAND-INJECTION`'
  Initialized empty Git repository in $TESTTMP/`echo pwned >COMMAND-INJECTION`/.git/
  $ cd '`echo pwned >COMMAND-INJECTION`'
  $ git commit -q --allow-empty -m 'empty'
  $ cd ..
  $ hg convert '`echo pwned >COMMAND-INJECTION`' 'converted'
  initializing destination converted repository
  scanning source...
  sorting...
  converting...
  0 empty
  updating bookmarks
  $ test -f COMMAND-INJECTION
  [1]

test for safely passing paths to git (CVE-2016-3105)

  $ git init 'ext::sh -c echo% pwned% >GIT-EXT-COMMAND-INJECTION% #'
  Initialized empty Git repository in $TESTTMP/ext::sh -c echo% pwned% >GIT-EXT-COMMAND-INJECTION% #/.git/
  $ cd 'ext::sh -c echo% pwned% >GIT-EXT-COMMAND-INJECTION% #'
  $ git commit -q --allow-empty -m 'empty'
  $ cd ..
  $ hg convert 'ext::sh -c echo% pwned% >GIT-EXT-COMMAND-INJECTION% #' 'converted-git-ext'
  initializing destination converted-git-ext repository
  scanning source...
  sorting...
  converting...
  0 empty
  updating bookmarks
  $ test -f GIT-EXT-COMMAND-INJECTION
  [1]

#endif

Conversion of extra commit metadata to extras works

  $ git init gitextras >/dev/null 2>/dev/null
  $ cd gitextras
  $ touch foo
  $ git add foo
  $ commit -m initial
  $ echo 1 > foo
  $ tree=`git write-tree`

Git doesn't provider a user-facing API to write extra metadata into the
commit, so create the commit object by hand

  $ git hash-object -t commit -w --stdin << EOF
  > tree ${tree}
  > parent ba6b1344e977ece9e00958dbbf17f1f09384b2c1
  > author test <test@example.com> 1000000000 +0000
  > committer test <test@example.com> 1000000000 +0000
  > extra-1 extra-1
  > extra-2 extra-2 with space
  > convert_revision 0000aaaabbbbccccddddeeee
  > 
  > message with extras
  > EOF
  8123727c8361a4117d1a2d80e0c4e7d70c757f18

  $ git reset --hard 8123727c8361a4117d1a2d80e0c4e7d70c757f18 > /dev/null

  $ cd ..

convert will not retain custom metadata keys by default

  $ hg convert gitextras hgextras1
  initializing destination hgextras1 repository
  scanning source...
  sorting...
  converting...
  1 initial
  0 message with extras
  updating bookmarks

  $ hg -R hgextras1 log --debug -r 1
  changeset:   1:e13a39880f68479127b2a80fa0b448cc8524aa09
  bookmark:    master
  phase:       draft
  parent:      0:dcb68977c55cd02cbd13b901df65c4b6e7b9c4b9
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50
  user:        test <test@example.com>
  date:        Sun Sep 09 01:46:40 2001 +0000
  extra:       branch=default
  extra:       convert_revision=8123727c8361a4117d1a2d80e0c4e7d70c757f18
  description:
  message with extras
  
  

Attempting to convert a banned extra is disallowed

  $ hg convert --config convert.git.extrakeys=tree,parent gitextras hgextras-banned
  initializing destination hgextras-banned repository
  abort: copying of extra key is forbidden: parent, tree
  [255]

Converting a specific extra works

  $ hg convert --config convert.git.extrakeys=extra-1 gitextras hgextras2
  initializing destination hgextras2 repository
  scanning source...
  sorting...
  converting...
  1 initial
  0 message with extras
  updating bookmarks

  $ hg -R hgextras2 log --debug -r 1
  changeset:   1:d40fb205d58597e6ecfd55b16f198be5bf436391
  bookmark:    master
  phase:       draft
  parent:      0:dcb68977c55cd02cbd13b901df65c4b6e7b9c4b9
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50
  user:        test <test@example.com>
  date:        Sun Sep 09 01:46:40 2001 +0000
  extra:       branch=default
  extra:       convert_revision=8123727c8361a4117d1a2d80e0c4e7d70c757f18
  extra:       extra-1=extra-1
  description:
  message with extras
  
  

Converting multiple extras works

  $ hg convert --config convert.git.extrakeys=extra-1,extra-2 gitextras hgextras3
  initializing destination hgextras3 repository
  scanning source...
  sorting...
  converting...
  1 initial
  0 message with extras
  updating bookmarks

  $ hg -R hgextras3 log --debug -r 1
  changeset:   1:0105af33379e7b6491501fd34141b7af700fe125
  bookmark:    master
  phase:       draft
  parent:      0:dcb68977c55cd02cbd13b901df65c4b6e7b9c4b9
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50
  user:        test <test@example.com>
  date:        Sun Sep 09 01:46:40 2001 +0000
  extra:       branch=default
  extra:       convert_revision=8123727c8361a4117d1a2d80e0c4e7d70c757f18
  extra:       extra-1=extra-1
  extra:       extra-2=extra-2 with space
  description:
  message with extras
  
  

convert.git.saverev can be disabled to prevent convert_revision from being written

  $ hg convert --config convert.git.saverev=false gitextras hgextras4
  initializing destination hgextras4 repository
  scanning source...
  sorting...
  converting...
  1 initial
  0 message with extras
  updating bookmarks

  $ hg -R hgextras4 log --debug -r 1
  changeset:   1:1dcaf4ffe5bee43fa86db2800821f6f0af212c5c
  bookmark:    master
  phase:       draft
  parent:      0:a13935fec4daf06a5a87a7307ccb0fc94f98d06d
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50
  user:        test <test@example.com>
  date:        Sun Sep 09 01:46:40 2001 +0000
  extra:       branch=default
  description:
  message with extras
  
  

convert.git.saverev and convert.git.extrakeys can be combined to preserve
convert_revision from source

  $ hg convert --config convert.git.saverev=false --config convert.git.extrakeys=convert_revision gitextras hgextras5
  initializing destination hgextras5 repository
  scanning source...
  sorting...
  converting...
  1 initial
  0 message with extras
  updating bookmarks

  $ hg -R hgextras5 log --debug -r 1
  changeset:   1:574d85931544d4542007664fee3747360e85ee28
  bookmark:    master
  phase:       draft
  parent:      0:a13935fec4daf06a5a87a7307ccb0fc94f98d06d
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    6a3df4de388f3c4f8e28f4f9a814299a3cbb5f50
  user:        test <test@example.com>
  date:        Sun Sep 09 01:46:40 2001 +0000
  extra:       branch=default
  extra:       convert_revision=0000aaaabbbbccccddddeeee
  description:
  message with extras
  
  
