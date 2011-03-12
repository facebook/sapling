  $ hg init outer
  $ cd outer

hg debugsub with no remapping

  $ echo 'sub = http://example.net/libfoo' > .hgsub
  $ hg add .hgsub

  $ hg debugsub
  path sub
   source   http://example.net/libfoo
   revision 

hg debugsub with remapping

  $ echo '[subpaths]' > .hg/hgrc
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

test bad subpaths pattern

  $ cat > .hg/hgrc <<EOF
  > [subpaths]
  > .* = \1
  > EOF
  $ hg debugsub
  abort: bad subrepository pattern in $TESTTMP/outer/.hg/hgrc:2: invalid group reference
  [255]
