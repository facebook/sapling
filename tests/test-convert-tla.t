#require tla symlink

  $ tla my-id "mercurial <mercurial@mercurial-scm.org>"
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH

create tla archive

  $ tla make-archive tla@mercurial--convert `pwd`/hg-test-convert-tla

initialize tla repo

  $ mkdir tla-repo
  $ cd tla-repo/
  $ tla init-tree tla@mercurial--convert/tla--test--0
  $ tla import
  * creating version tla@mercurial--convert/tla--test--0
  * imported tla@mercurial--convert/tla--test--0

create initial files

  $ echo 'this is a file' > a
  $ tla add a
  $ mkdir src
  $ tla add src
  $ cd src
  $ dd count=1 if=/dev/zero of=b > /dev/null 2> /dev/null
  $ tla add b
  $ tla commit -s "added a file, src and src/b (binary)"
  A/ .arch-ids
  A/ src
  A/ src/.arch-ids
  A  .arch-ids/a.id
  A  a
  A  src/.arch-ids/=id
  A  src/.arch-ids/b.id
  A  src/b
  * update pristine tree (tla@mercurial--convert/tla--test--0--base-0 => tla--test--0--patch-1)
  * committed tla@mercurial--convert/tla--test--0--patch-1

create link file and modify a

  $ ln -s ../a a-link
  $ tla add a-link
  $ echo 'this a modification to a' >> ../a
  $ tla commit -s "added link to a and modify a"
  A  src/.arch-ids/a-link.id
  A  src/a-link
  M  a
  * update pristine tree (tla@mercurial--convert/tla--test--0--patch-1 => tla--test--0--patch-2)
  * committed tla@mercurial--convert/tla--test--0--patch-2

create second link and modify b

  $ ln -s ../a a-link-2
  $ tla add a-link-2
  $ dd count=1 seek=1 if=/dev/zero of=b > /dev/null 2> /dev/null
  $ tla commit -s "added second link and modify b"
  A  src/.arch-ids/a-link-2.id
  A  src/a-link-2
  Mb src/b
  * update pristine tree (tla@mercurial--convert/tla--test--0--patch-2 => tla--test--0--patch-3)
  * committed tla@mercurial--convert/tla--test--0--patch-3

b file to link and a-link-2 to regular file

  $ rm -f a-link-2
  $ echo 'this is now a regular file' > a-link-2
  $ ln -sf ../a b
  $ tla commit -s "file to link and link to file test"
  fl src/b
  lf src/a-link-2
  * update pristine tree (tla@mercurial--convert/tla--test--0--patch-3 => tla--test--0--patch-4)
  * committed tla@mercurial--convert/tla--test--0--patch-4

move a-link-2 file and src directory

  $ cd ..
  $ tla mv src/a-link-2 c
  $ tla mv src test
  $ tla commit -s "move and rename a-link-2 file and src directory"
  D/ src/.arch-ids
  A/ test/.arch-ids
  /> src	test
  => src/.arch-ids/a-link-2.id	.arch-ids/c.id
  => src/a-link-2	c
  => src/.arch-ids/=id	test/.arch-ids/=id
  => src/.arch-ids/a-link.id	test/.arch-ids/a-link.id
  => src/.arch-ids/b.id	test/.arch-ids/b.id
  * update pristine tree (tla@mercurial--convert/tla--test--0--patch-4 => tla--test--0--patch-5)
  * committed tla@mercurial--convert/tla--test--0--patch-5
  $ cd ..

converting tla repo to Mercurial

  $ hg convert tla-repo tla-repo-hg
  initializing destination tla-repo-hg repository
  analyzing tree version tla@mercurial--convert/tla--test--0...
  scanning source...
  sorting...
  converting...
  5 initial import
  4 added a file, src and src/b (binary)
  3 added link to a and modify a
  2 added second link and modify b
  1 file to link and link to file test
  0 move and rename a-link-2 file and src directory
  $ tla register-archive -d tla@mercurial--convert
  $ glog()
  > {
  >     hg log -G --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
  > }

show graph log

  $ glog -R tla-repo-hg
  o  5 "move and rename a-link-2 file and src directory" files: c src/a-link src/a-link-2 src/b test/a-link test/b
  |
  o  4 "file to link and link to file test" files: src/a-link-2 src/b
  |
  o  3 "added second link and modify b" files: src/a-link-2 src/b
  |
  o  2 "added link to a and modify a" files: a src/a-link
  |
  o  1 "added a file, src and src/b (binary)" files: a src/b
  |
  o  0 "initial import" files:
  
  $ hg up -q -R tla-repo-hg
  $ hg -R tla-repo-hg manifest --debug
  c4072c4b72e1cabace081888efa148ee80ca3cbb 644   a
  0201ac32a3a8e86e303dff60366382a54b48a72e 644   c
  c0067ba5ff0b7c9a3eb17270839d04614c435623 644 @ test/a-link
  375f4263d86feacdea7e3c27100abd1560f2a973 644 @ test/b
