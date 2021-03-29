#chg-compatible

Test the lfs.placeholders config option

  $ enable lfs

  $ newrepo server
  $ setconfig lfs.url=file://$TESTTMP/remote lfs.threshold=8
  $ drawdag <<'EOS'
  >  B2 # B2/B2=lots_of_text
  >  |
  >  A1 # A1/A1=small
  > EOS

One commit has LFS file (flag=2000)

  $ hg debugfilerevision -r 'all()'
  3828e4693c74: A1
   A1: bin=0 lnk=0 flag=0 size=5 copied='' chain=0cfbf921b2cb
  7e1c7b2cd9df: B2
   B2: bin=0 lnk=0 flag=2000 size=12 copied='' chain=eacc6746870c

  $ hg debuglfsupload -r 'all()'

Clone the repo
  $ cd ..
  $ hg clone --config experimental.lfsplaceholders=True server client
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ setconfig experimental.lfsplaceholders=True

Demonstrate that placeholders are there
  $ cat B2
  This is a placeholder for a large file
  
  Original file id: sha256:88cab35a00c697e745f11131c19eac3a078683dc4d06f840cb9b40aa010cb29c
  Original file size: 12
  Original file is binary: False

Demonstrate that non-LFS file is there
  $ cat A1
  small (no-eol)

Diff and status should be clean
  $ hg diff
  $ hg status

Committing new files should be possible only when they are below LFS treshold
  $ setconfig lfs.threshold=8
  $ echo "tiny" > tinyfile
  $ hg commit -Aq -m tiny

  $ echo "very large file" > verylargefile
  $ hg commit -Aq -m verylargefile
  abort: can't write LFS files in placeholders mode
  [255]
  $ rm verylargefile

Disable the placeholders mode
  $ setconfig experimental.lfsplaceholders=False
  $ setconfig lfs.url=file://$TESTTMP/remote lfs.threshold=8

Recrawling distate is neccessary
  $ hg debugrebuilddirstate

File should be dirty now
  $ hg diff
  diff -r ad054566f884 B2
  --- a/B2	Thu Jan 01 00:00:00 1970 +0000
  +++ b/B2	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,5 @@
  -lots_of_text
  \ No newline at end of file
  +This is a placeholder for a large file
  +
  +Original file id: sha256:88cab35a00c697e745f11131c19eac3a078683dc4d06f840cb9b40aa010cb29c
  +Original file size: 12
  +Original file is binary: False
  $ hg status
  M B2
  $ hg update -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat B2
  lots_of_text (no-eol)
