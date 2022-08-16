#chg-compatible
#debugruntest-compatible

test clone with lazy pull

  $ configure modern
  $ setconfig paths.default=test:e1
  $ setconfig clone.nativepull=True

Prepare Repo:

  $ newremoterepo repo1
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > E
  > |
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ hg push -r $E --to master --create -q
  $ hg push -r $C --to stable --create -q

Clone the lazy repo, pulling master and stable bookmarks:

  $ hg clone -U --shallow test:e1 --config remotefilelog.reponame=x $TESTTMP/cloned1 --config remotenames.selectivepulldefault="master, stable" -q

  $ cd $TESTTMP/cloned1

Check clone data import
  $ hg log -T "{desc} {node|short}\n"
  E 9bc730a19041
  D f585351a92f8
  C 26805aba1e60
  B 112478962961
  A 426bada5c675

Check remotenames and tip are written correctly
  $ hg book --all
  no bookmarks set
     remote/master             9bc730a19041
     remote/stable             26805aba1e60
  $ hg log -r tip -T "{node|short}"
  9bc730a19041 (no-eol)
