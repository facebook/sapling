#chg-compatible

  $ enable remotenames

Init a repo

  $ hg init pathsrepo
  $ cd pathsrepo

Check that a new path can be added

  $ hg paths -a yellowbrickroad yellowbrickroad
  $ hg paths -a stairwaytoheaven stairwaytoheaven
  $ hg paths
  stairwaytoheaven = $TESTTMP/pathsrepo/stairwaytoheaven
  yellowbrickroad = $TESTTMP/pathsrepo/yellowbrickroad

Check that a repo can be deleted

  $ hg paths -d yellowbrickroad
  $ hg paths
  stairwaytoheaven = $TESTTMP/pathsrepo/stairwaytoheaven

Delete .hg/hgrc fil

  $ rm .hg/hgrc

Check that a path cannot be deleted when no .hg/hgrc file exists

  $ hg paths -d stairwaytoheaven
  abort: could not find hgrc file
  [255]

Check that a path can be added when no .hg/hgrc file exists

  $ hg paths -a yellowbrickroad yellowbrickroad
  $ hg paths
  yellowbrickroad = $TESTTMP/pathsrepo/yellowbrickroad

