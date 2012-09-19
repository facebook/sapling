Init repo1:

  $ hg init repo1
  $ cd repo1
  $ echo "some text" > a
  $ hg add
  adding a
  $ hg ci -m first
  $ cat .hg/store/fncache | sort
  data/a.i

Testing a.i/b:

  $ mkdir a.i
  $ echo "some other text" > a.i/b
  $ hg add
  adding a.i/b (glob)
  $ hg ci -m second
  $ cat .hg/store/fncache | sort
  data/a.i
  data/a.i.hg/b.i

Testing a.i.hg/c:

  $ mkdir a.i.hg
  $ echo "yet another text" > a.i.hg/c
  $ hg add
  adding a.i.hg/c (glob)
  $ hg ci -m third
  $ cat .hg/store/fncache | sort
  data/a.i
  data/a.i.hg.hg/c.i
  data/a.i.hg/b.i

Testing verify:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 3 changesets, 3 total revisions

  $ rm .hg/store/fncache

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   data/a.i@0: missing revlog!
   data/a.i.hg/c.i@2: missing revlog!
   data/a.i/b.i@1: missing revlog!
  3 files, 3 changesets, 3 total revisions
  3 integrity errors encountered!
  (first damaged changeset appears to be 0)
  [1]
  $ cd ..

Non store repo:

  $ hg --config format.usestore=False init foo
  $ cd foo
  $ mkdir tst.d
  $ echo foo > tst.d/foo
  $ hg ci -Amfoo
  adding tst.d/foo
  $ find .hg | sort
  .hg
  .hg/00changelog.i
  .hg/00manifest.i
  .hg/cache
  .hg/cache/branchheads
  .hg/data
  .hg/data/tst.d.hg
  .hg/data/tst.d.hg/foo.i
  .hg/dirstate
  .hg/last-message.txt
  .hg/phaseroots
  .hg/requires
  .hg/undo
  .hg/undo.bookmarks
  .hg/undo.branch
  .hg/undo.desc
  .hg/undo.dirstate
  .hg/undo.phaseroots
  $ cd ..

Non fncache repo:

  $ hg --config format.usefncache=False init bar
  $ cd bar
  $ mkdir tst.d
  $ echo foo > tst.d/Foo
  $ hg ci -Amfoo
  adding tst.d/Foo
  $ find .hg | sort
  .hg
  .hg/00changelog.i
  .hg/cache
  .hg/cache/branchheads
  .hg/dirstate
  .hg/last-message.txt
  .hg/requires
  .hg/store
  .hg/store/00changelog.i
  .hg/store/00manifest.i
  .hg/store/data
  .hg/store/data/tst.d.hg
  .hg/store/data/tst.d.hg/_foo.i
  .hg/store/phaseroots
  .hg/store/undo
  .hg/store/undo.phaseroots
  .hg/undo.bookmarks
  .hg/undo.branch
  .hg/undo.desc
  .hg/undo.dirstate
  $ cd ..

#if no-windows

Encoding of reserved / long paths in the store

  $ hg init r2
  $ cd r2
  $ cat <<EOF > .hg/hgrc
  > [ui]
  > portablefilenames = ignore
  > EOF

  $ DIR="bla.aux/prn/PRN/lpt/com3/nul/coma/foo.NUL"
  $ mkdir -p "$DIR"
  $ echo foo > "$DIR/normal.c"
  $ DIR="AUX/SECOND/X.PRN/FOURTH/FI:FTH/SIXTH/SEVENTH/EIGHTH/NINETH/TENTH/ELEVENTH"
  $ mkdir -p "$DIR"
  $ echo foo > "$DIR/LOREMIPSUM.TXT"
  $ DIR="enterprise/openesbaddons/contrib-imola/corba-bc/netbeansplugin/wsdlExtension/src/main/java/META-INF/services"
  $ mkdir -p "$DIR"
  $ echo foo > "$DIR/org.netbeans.modules.xml.wsdl.bindingsupport.spi.ExtensibilityElementTemplateProvider"
  $ DIR="Project Planning/Resources/AnotherLongDirectoryName/Followedbyanother/AndAnother"
  $ mkdir -p "$DIR"
  $ echo foo > "$DIR/AndThenAnExtremelyLongFileName.txt"
  $ DIR="12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345"
  $ mkdir -p "$DIR"
  $ echo foo > "$DIR/xxxxxxxxx-xxxxxxxxx-xxxxxxxxx-123456789-12.3456789-12345-ABCDEFGHIJKLMNOPRSTUVWXYZ-abcdefghjiklmnopqrstuvwxyz"
  $ hg ci -qAm1
  $ find .hg/store -name *.i  | sort
  .hg/store/00changelog.i
  .hg/store/00manifest.i
  .hg/store/data/bla.aux/pr~6e/_p_r_n/lpt/co~6d3/nu~6c/coma/foo._n_u_l/normal.c.i
  .hg/store/dh/12345678/12345678/12345678/12345678/12345678/12345678/12345678/12345/xxxxxx168e07b38e65eff86ab579afaaa8e30bfbe0f35f.i
  .hg/store/dh/au~78/second/x.prn/fourth/fi~3afth/sixth/seventh/eighth/nineth/tenth/loremia20419e358ddff1bf8751e38288aff1d7c32ec05.i
  .hg/store/dh/enterpri/openesba/contrib-/corba-bc/netbeans/wsdlexte/src/main/java/org.net7018f27961fdf338a598a40c4683429e7ffb9743.i
  .hg/store/dh/project_/resource/anotherl/followed/andanoth/andthenanextremelylongfilename0d8e1f4187c650e2f1fdca9fd90f786bc0976b6b.i

  $ cd ..

#endif

