  > echo "[extensions]" >> $HGRCPATH
  > echo "remotenames=`dirname $TESTDIR`/remotenames.py" >> $HGRCPATH

  > FILTERPWD="sed s%`pwd`/%%g"

  > mkcommit()
  > {
  >    echo $1 > $1
  >    hg add $1
  >    hg ci -m "add $1"
  > }

Test that remotenames works on a repo without any names file

  $ hg init alpha
  $ cd alpha
  $ mkcommit a
  $ mkcommit b
  $ hg log -r 'upstream()'
  $ hg log -r . -T '{remotenames} {remotebranches} {remotebookmarks}\n'
    

Continue testing

  $ hg branch stable
  marked working directory as branch stable
  (branches are permanent and global, did you want a bookmark?)
  $ mkcommit c
  $ cd ..
  $ hg clone alpha beta | $FILTERPWD
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd beta
  $ hg book babar
  $ mkcommit d
  $ hg co -C stable
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark babar)
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merged'
  $ cd ..

  $ hg init gamma
  $ cd gamma
  $ cat > .hg/hgrc <<EOF
  > [paths]
  > default = ../alpha
  > alpha = ../alpha
  > beta = ../beta
  > EOF
  $ hg pull | $FILTERPWD
  pulling from alpha
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files
  (run 'hg update' to get a working copy)
  $ hg pull beta | $FILTERPWD
  pulling from beta
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg co -C default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch default
  marked working directory as branch default
  $ mkcommit e
  $ hg merge stable
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m 'merging stable'

graph shows tags for the branch heads of each path
  $ hg log --graph
  @    changeset:   6:ce61ec32ee23
  |\   tag:         tip
  | |  parent:      5:6d6442577283
  | |  parent:      4:8948da77173b
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merging stable
  | |
  | o  changeset:   5:6d6442577283
  | |  parent:      3:78f83396d79e
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add e
  | |
  o |  changeset:   4:8948da77173b
  |\|  branch:      stable
  | |  branch:      beta/stable
  | |  parent:      2:95cb4ab9fe1d
  | |  parent:      3:78f83396d79e
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merged
  | |
  | o  changeset:   3:78f83396d79e
  | |  bookmark:    beta/babar
  | |  branch:      beta/default
  | |  parent:      1:7c3bad9141dc
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add d
  | |
  o |  changeset:   2:95cb4ab9fe1d
  |/   branch:      stable
  |    branch:      alpha/stable
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     add c
  |
  o  changeset:   1:7c3bad9141dc
  |  branch:      alpha/default
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add b
  |
  o  changeset:   0:1f0dee641bb7
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     add a
  

make sure we can list remote bookmarks with --all

  $ hg bookmarks --all
  no bookmarks set
     beta/babar                3:78f83396d79e

  $ hg bookmarks --remote
     beta/babar                3:78f83396d79e

  $ hg branches --all
  default                        6:ce61ec32ee23
  stable                         4:8948da77173b (inactive)
  beta/stable                    4:8948da77173b
  beta/default                   3:78f83396d79e
  alpha/stable                   2:95cb4ab9fe1d
  alpha/default                  1:7c3bad9141dc

  $ hg branches --remote
  beta/stable                    4:8948da77173b
  beta/default                   3:78f83396d79e
  alpha/stable                   2:95cb4ab9fe1d
  alpha/default                  1:7c3bad9141dc

make sure bogus revisions in .hg/remotenames do not break hg
  $ echo deadbeefdeadbeefdeadbeefdeadbeefdeadbeef default/default >> \
  > .hg/remotenames
  $ hg parents
  changeset:   6:ce61ec32ee23
  tag:         tip
  parent:      5:6d6442577283
  parent:      4:8948da77173b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merging stable
  
Verify that the revsets operate as expected:
  $ hg log -r 'not pushed()'
  changeset:   5:6d6442577283
  parent:      3:78f83396d79e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  
  changeset:   6:ce61ec32ee23
  tag:         tip
  parent:      5:6d6442577283
  parent:      4:8948da77173b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merging stable
  


Upstream without configuration is synonymous with upstream('default'):
  $ hg log -r 'not upstream()'
  changeset:   3:78f83396d79e
  bookmark:    beta/babar
  branch:      beta/default
  parent:      1:7c3bad9141dc
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add d
  
  changeset:   4:8948da77173b
  branch:      stable
  branch:      beta/stable
  parent:      2:95cb4ab9fe1d
  parent:      3:78f83396d79e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merged
  
  changeset:   5:6d6442577283
  parent:      3:78f83396d79e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add e
  
  changeset:   6:ce61ec32ee23
  tag:         tip
  parent:      5:6d6442577283
  parent:      4:8948da77173b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merging stable
  

but configured, it'll do the expected thing:
  $ echo '[remotenames]' >> .hg/hgrc
  $ echo 'upstream=alpha' >> .hg/hgrc
  $ hg log --graph -r 'not upstream()'
  @    changeset:   6:ce61ec32ee23
  |\   tag:         tip
  | |  parent:      5:6d6442577283
  | |  parent:      4:8948da77173b
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merging stable
  | |
  | o  changeset:   5:6d6442577283
  | |  parent:      3:78f83396d79e
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add e
  | |
  o |  changeset:   4:8948da77173b
  |\|  branch:      stable
  | |  branch:      beta/stable
  | |  parent:      2:95cb4ab9fe1d
  | |  parent:      3:78f83396d79e
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     merged
  | |
  | o  changeset:   3:78f83396d79e
  | |  bookmark:    beta/babar
  | |  branch:      beta/default
  | |  parent:      1:7c3bad9141dc
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     add d
  | |
  $ hg log --limit 2 --graph -r 'heads(upstream())'
  o  changeset:   2:95cb4ab9fe1d
  |  branch:      stable
  |  branch:      alpha/stable
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     add c
  |

Test remotenames revset and keyword

  $ hg log -r 'remotenames()' \
  >   --template '{rev}:{node|short} {remotenames}\n'
  1:7c3bad9141dc alpha/default
  2:95cb4ab9fe1d alpha/stable
  3:78f83396d79e beta/babar beta/default
  4:8948da77173b beta/stable

Test clone --mirror

  $ cd ..
  $ cd alpha
  $ hg book foo bar baz
  $ cd ..
  $ hg clone --mirror alpha mirror
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd mirror
  $ hg book
     bar                       2:95cb4ab9fe1d
     baz                       2:95cb4ab9fe1d
     foo                       2:95cb4ab9fe1d

Test loading with hggit

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "hggit=" >> $HGRCPATH
  $ hg help bookmarks  | grep -A 3 -- '--track'
   -t --track BOOKMARK track this bookmark or remote name
   -u --untrack        remove tracking for this bookmark
   -a --all            show both remote and local bookmarks
      --remote         show only remote bookmarks

Test branches marked as closed are not loaded
  $ cd ../alpha
  $ hg branch
  stable
  $ hg commit --close-branch -m 'close this branch'

  $ cd ../beta
  $ hg branches --remote
  default/stable                 2:95cb4ab9fe1d
  default/default                1:7c3bad9141dc
  $ hg pull -q
  $ hg branches --remote
  default/default                1:7c3bad9141dc

Test json formatted bookmarks with tracking data
  $ cd ..
  $ hg init delta
  $ cd delta
  $ hg book mimimi -t lalala
  $ hg book -v -T json
  [
   {
    "active": true,
    "bookmark": "mimimi",
    "node": "0000000000000000000000000000000000000000",
    "rev": -1,
    "tracking": "lalala"
   }
  ]
  $ hg book -v
   * mimimi                    -1:000000000000           [lalala]
