  $ branches=.hg/cache/branchheads
  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'mq =' >> $HGRCPATH

  $ show_branch_cache()
  > {
  >     # force cache (re)generation
  >     hg log -r does-not-exist 2> /dev/null
  >     hg log -r tip --template 'tip: {rev}\n'
  >     if [ -f $branches ]; then
  >       sort $branches
  >     else
  >       echo No branch cache
  >     fi
  >     if [ "$1" = 1 ]; then
  >       for b in foo bar; do
  >         hg log -r $b --template "branch $b: "'{rev}\n'
  >       done
  >     fi
  > }

  $ hg init a
  $ cd a
  $ hg qinit -c


mq patch on an empty repo

  $ hg qnew p1
  $ show_branch_cache
  tip: 0
  No branch cache

  $ echo > pfile
  $ hg add pfile
  $ hg qrefresh -m 'patch 1'
  $ show_branch_cache
  tip: 0
  No branch cache

some regular revisions

  $ hg qpop
  popping p1
  patch queue now empty
  $ echo foo > foo
  $ hg add foo
  $ echo foo > .hg/branch
  $ hg ci -m 'branch foo'

  $ echo bar > bar
  $ hg add bar
  $ echo bar > .hg/branch
  $ hg ci -m 'branch bar'
  $ show_branch_cache
  tip: 1
  c229711f16da3d7591f89b1b8d963b79bda22714 1
  c229711f16da3d7591f89b1b8d963b79bda22714 bar
  dc25e3827021582e979f600811852e36cbe57341 foo

add some mq patches

  $ hg qpush
  applying p1
  now at: p1
  $ show_branch_cache
  tip: 2
  c229711f16da3d7591f89b1b8d963b79bda22714 1
  c229711f16da3d7591f89b1b8d963b79bda22714 bar
  dc25e3827021582e979f600811852e36cbe57341 foo

  $ hg qnew p2
  $ echo foo > .hg/branch
  $ echo foo2 >> foo
  $ hg qrefresh -m 'patch 2'
  $ show_branch_cache 1
  tip: 3
  c229711f16da3d7591f89b1b8d963b79bda22714 1
  c229711f16da3d7591f89b1b8d963b79bda22714 bar
  dc25e3827021582e979f600811852e36cbe57341 foo
  branch foo: 3
  branch bar: 2

removing the cache

  $ rm $branches
  $ show_branch_cache 1
  tip: 3
  c229711f16da3d7591f89b1b8d963b79bda22714 1
  c229711f16da3d7591f89b1b8d963b79bda22714 bar
  dc25e3827021582e979f600811852e36cbe57341 foo
  branch foo: 3
  branch bar: 2

importing rev 1 (the cache now ends in one of the patches)

  $ hg qimport -r 1 -n p0
  $ show_branch_cache 1
  tip: 3
  c229711f16da3d7591f89b1b8d963b79bda22714 1
  c229711f16da3d7591f89b1b8d963b79bda22714 bar
  dc25e3827021582e979f600811852e36cbe57341 foo
  branch foo: 3
  branch bar: 2
  $ hg log -r qbase --template 'qbase: {rev}\n'
  qbase: 1

detect an invalid cache

  $ hg qpop -a
  popping p2
  popping p1
  popping p0
  patch queue now empty
  $ hg qpush -a
  applying p0
  applying p1
  applying p2
  now at: p2
  $ show_branch_cache
  tip: 3
  dc25e3827021582e979f600811852e36cbe57341 0
  dc25e3827021582e979f600811852e36cbe57341 foo

