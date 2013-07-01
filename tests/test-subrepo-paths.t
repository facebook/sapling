  $ hg init outer
  $ cd outer

  $ echo '[paths]' >> .hg/hgrc
  $ echo 'default = http://example.net/' >> .hg/hgrc

hg debugsub with no remapping

  $ echo 'sub = libfoo' > .hgsub
  $ hg add .hgsub

  $ hg debugsub
  path sub
   source   libfoo
   revision 

hg debugsub with remapping

  $ echo '[subpaths]' >> .hg/hgrc
  $ printf 'http://example.net/lib(.*) = C:\\libs\\\\1-lib\\\n' >> .hg/hgrc

  $ hg debugsub
  path sub
   source   C:\libs\foo-lib\
   revision 

test cumulative remapping, the $HGRCPATH file is loaded first

  $ echo '[subpaths]' >> $HGRCPATH
  $ echo 'libfoo = libbar' >> $HGRCPATH
  $ hg debugsub
  path sub
   source   C:\libs\bar-lib\
   revision 

test absolute source path -- testing with a URL is important since
standard os.path.join wont treat that as an absolute path

  $ echo 'abs = http://example.net/abs' > .hgsub
  $ hg debugsub
  path abs
   source   http://example.net/abs
   revision 

  $ echo 'abs = /abs' > .hgsub
  $ hg debugsub
  path abs
   source   /abs
   revision 

test bad subpaths pattern

  $ cat > .hg/hgrc <<EOF
  > [subpaths]
  > .* = \1
  > EOF
  $ hg debugsub
  abort: bad subrepository pattern in $TESTTMP/outer/.hg/hgrc:2: invalid group reference (glob)
  [255]

  $ cd ..
