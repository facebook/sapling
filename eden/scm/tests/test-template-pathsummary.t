  $ eagerepo
  $ newrepo
  $ setconfig templatealias.summary='"{pathsummary(file_mods, 3) % \"M {path}\n\"}{pathsummary(file_adds, 3) % \"A {path}\n\"}{pathsummary(file_dels, 3) % \"R {path}\"}"'

  $ mkfiles() {
  >   for f in "$@"
  >   do
  >     mkdir -p "$(dirname $f)"
  >     echo . >> $f
  >   done
  > }

  $ mkfiles file{0,1,2} dir1/file3 dir2/file{4,5,6,7} dir3/dir4/file8
  $ hg addremove -q
  $ hg commit -m commit1
  $ mkfiles file1 dir1/file3 dir2/file{4,5,6} dir2/file9
  $ rm file2
  $ hg addremove -q
  $ hg commit -m commit2
  $ mkfiles dir3/dir4/file{8,9} dir3/dir5/file{10,11} dir3/dir6/file{12,13,14} dir3/dir6/file15
  $ rm -rf dir2
  $ hg addremove -q
  $ hg commit -m commit3
  $ mkfiles file{0,1} dir1/file3 dir3/dir6/file15
  $ hg addremove -q
  $ hg commit -m commit4

  $ hg log -G -T '{desc}\n{summary}'
  @  commit4
  │  M dir1/file3
  │  M dir3/dir6/file15
  │  M (2 files)
  o  commit3
  │  M dir3/dir4/file8
  │  A dir3/… (7 files)
  │  R dir2/ (5 files)
  o  commit2
  │  M dir1/file3
  │  M dir2/ (3 files)
  │  M file1
  │  A dir2/file9
  │  R file2
  o  commit1
     A … (9 files)
