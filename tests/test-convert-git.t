
  $ "$TESTDIR/hghave" git || exit 80
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ echo 'hgext.graphlog =' >> $HGRCPATH
  $ GIT_AUTHOR_NAME='test'; export GIT_AUTHOR_NAME
  $ GIT_AUTHOR_EMAIL='test@example.org'; export GIT_AUTHOR_EMAIL
  $ GIT_AUTHOR_DATE="2007-01-01 00:00:00 +0000"; export GIT_AUTHOR_DATE
  $ GIT_COMMITTER_NAME="$GIT_AUTHOR_NAME"; export GIT_COMMITTER_NAME
  $ GIT_COMMITTER_EMAIL="$GIT_AUTHOR_EMAIL"; export GIT_COMMITTER_EMAIL
  $ GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"; export GIT_COMMITTER_DATE
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

Remove the directory, then try to replace it with a file
(issue 754)

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
  $ hg convert --datesort git-repo
  assuming destination git-repo-hg
  initializing destination git-repo-hg repository
  scanning source...
  sorting...
  converting...
  5 t1
  4 t2
  3 t3
  2 t4.1
  1 t4.2
  0 Merge branch other
  $ hg up -q -R git-repo-hg
  $ hg -R git-repo-hg tip -v
  changeset:   5:c78094926be2
  tag:         tip
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
  >     hg glog --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
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

  $ hg -q convert --datesort git-repo2 fullrepo
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
  $ echo
  

test binary conversion (issue 1359)

  $ mkdir git-repo3
  $ cd git-repo3
  $ git init-db >/dev/null 2>/dev/null
  $ python -c 'file("b", "wb").write("".join([chr(i) for i in range(256)])*16)'
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
  $ cd git-repo3-hg
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ python -c 'print len(file("b", "rb").read())'
  4096
  $ cd ..
  $ echo
  

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
  $ hg -R git-repo4-hg log -v
  changeset:   1:d63e967f93da
  tag:         tip
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
  
  

--sourceorder should fail

  $ hg convert --sourcesort git-repo4 git-repo4-sourcesort-hg
  initializing destination git-repo4-sourcesort-hg repository
  abort: --sourcesort is not supported by this data source
  [255]

damage git repository and convert again

  $ cat > damage.py <<EOF
  > import os
  > for root, dirs, files in os.walk('git-repo4/.git/objects'):
  >     if files:
  >         path = os.path.join(root, files[0])
  >         os.remove(path)
  >         break
  > EOF
  $ python damage.py
  $ hg convert git-repo4 git-repo4-broken-hg 2>&1 | \
  >     grep 'abort:' | sed 's/abort:.*/abort:/g'
  abort:
