#chg-compatible
#debugruntest-compatible

#require symlink tar unzip

  $ newrepo repo
  $ ln -s nothing dangling

avoid tar warnings about old timestamp

  $ hg ci -d '2000-01-01 00:00:00 +0000' -qAm 'add symlink'

  $ hg archive -t files ../archive
  $ hg archive -t tar -p tar ../archive.tar
  $ hg archive -t zip -p zip ../archive.zip

files

  $ cd "$TESTTMP"
  $ cd archive
  $ f dangling
  dangling -> nothing

tar

  $ cd "$TESTTMP"
  $ tar xf archive.tar
  $ cd tar
  $ f dangling
  dangling -> nothing

#if unziplinks
zip

  $ cd "$TESTTMP"
  $ unzip archive.zip > /dev/null 2>&1
  $ cd zip
  $ f dangling
  dangling -> nothing
#endif

  $ cd ..
