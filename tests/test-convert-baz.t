#require baz symlink

  $ baz my-id "mercurial <mercurial@selenic.com>"

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH

create baz archive
  $ baz make-archive baz@mercurial--convert hg-test-convert-baz

initialize baz repo
  $ mkdir baz-repo
  $ cd baz-repo/
  $ baz init-tree baz@mercurial--convert/baz--test--0
  $ baz import
  * creating version baz@mercurial--convert/baz--test--0
  * imported baz@mercurial--convert/baz--test--0

create initial files
  $ echo 'this is a file' > a
  $ baz add a
  $ mkdir src
  $ baz add src
  $ cd src
  $ dd count=1 if=/dev/zero of=b > /dev/null 2> /dev/null
  $ baz add b
HACK: hide GNU tar-1.22 "tar: The --preserve option is deprecated, use --preserve-permissions --preserve-order instead"
  $ baz commit -s "added a file, src and src/b (binary)" 2>&1 | grep -v '^tar'
  * build pristine tree for baz@mercurial--convert/baz--test--0--base-0
  * Scanning for full-tree revision: .
  * from import revision: baz@mercurial--convert/baz--test--0--base-0
  A/ .arch-ids
  A/ src
  A/ src/.arch-ids
  A  .arch-ids/a.id
  A  a
  A  src/.arch-ids/=id
  A  src/.arch-ids/b.id
  A  src/b
  * update pristine tree (baz@mercurial--convert/baz--test--0--base-0 => baz--test--0--patch-1)
  * committed baz@mercurial--convert/baz--test--0--patch-1

create link file and modify a
  $ ln -s ../a a-link
  $ baz add a-link
  $ echo 'this a modification to a' >> ../a
  $ baz commit -s "added link to a and modify a"
  A  src/.arch-ids/a-link.id
  A  src/a-link
  M  a
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-1 => baz--test--0--patch-2)
  * committed baz@mercurial--convert/baz--test--0--patch-2

create second link and modify b
  $ ln -s ../a a-link-2
  $ baz add a-link-2
  $ dd count=1 seek=1 if=/dev/zero of=b > /dev/null 2> /dev/null
  $ baz commit -s "added second link and modify b"
  A  src/.arch-ids/a-link-2.id
  A  src/a-link-2
  Mb src/b
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-2 => baz--test--0--patch-3)
  * committed baz@mercurial--convert/baz--test--0--patch-3

b file to link and a-link-2 to regular file
  $ rm -f a-link-2
  $ echo 'this is now a regular file' > a-link-2
  $ ln -sf ../a b
  $ baz commit -s "file to link and link to file test"
  fl src/b
  lf src/a-link-2
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-3 => baz--test--0--patch-4)
  * committed baz@mercurial--convert/baz--test--0--patch-4

move a-link-2 file and src directory
  $ cd ..
  $ baz mv src/a-link-2 c
  $ baz mv src test
  $ baz commit -s "move and rename a-link-2 file and src directory"
  D/ src/.arch-ids
  A/ test/.arch-ids
  /> src	test
  => src/.arch-ids/a-link-2.id	.arch-ids/c.id
  => src/a-link-2	c
  => src/.arch-ids/=id	test/.arch-ids/=id
  => src/.arch-ids/a-link.id	test/.arch-ids/a-link.id
  => src/.arch-ids/b.id	test/.arch-ids/b.id
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-4 => baz--test--0--patch-5)
  * committed baz@mercurial--convert/baz--test--0--patch-5

move and add the moved file again
  $ echo e > e
  $ baz add e
  $ baz commit -s "add e"
  A  .arch-ids/e.id
  A  e
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-5 => baz--test--0--patch-6)
  * committed baz@mercurial--convert/baz--test--0--patch-6
  $ baz mv e f
  $ echo ee > e
  $ baz add e
  $ baz commit -s "move e and recreate it again"
  A  .arch-ids/e.id
  A  e
  => .arch-ids/e.id	.arch-ids/f.id
  => e	f
  * update pristine tree (baz@mercurial--convert/baz--test--0--patch-6 => baz--test--0--patch-7)
  * committed baz@mercurial--convert/baz--test--0--patch-7
  $ cd ..

converting baz repo to Mercurial
  $ hg convert baz-repo baz-repo-hg
  initializing destination baz-repo-hg repository
  analyzing tree version baz@mercurial--convert/baz--test--0...
  scanning source...
  sorting...
  converting...
  7 initial import
  6 added a file, src and src/b (binary)
  5 added link to a and modify a
  4 added second link and modify b
  3 file to link and link to file test
  2 move and rename a-link-2 file and src directory
  1 add e
  0 move e and recreate it again

  $ baz register-archive -d baz@mercurial--convert

  $ glog()
  > {
  >     hg log -G --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
  > }

show graph log
  $ glog -R baz-repo-hg
  o  7 "move e and recreate it again" files: e f
  |
  o  6 "add e" files: e
  |
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
  
  $ hg up -q -R baz-repo-hg
  $ hg -R baz-repo-hg manifest --debug
  c4072c4b72e1cabace081888efa148ee80ca3cbb 644   a
  0201ac32a3a8e86e303dff60366382a54b48a72e 644   c
  1a4a864db0073705a11b1439f563bfa4b46d9246 644   e
  09e0222742fc3f75777fa9d68a5d8af7294cb5e7 644   f
  c0067ba5ff0b7c9a3eb17270839d04614c435623 644 @ test/a-link
  375f4263d86feacdea7e3c27100abd1560f2a973 644 @ test/b
  $ hg -R baz-repo-hg log -r 5 -r 7 -C --debug | grep copies
  copies:      c (src/a-link-2) test/a-link (src/a-link) test/b (src/b)
  copies:      f (e)
