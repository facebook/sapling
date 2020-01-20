  $ disable treemanifest
  $ . "$TESTDIR/hggit/testutil"

  $ git init -q gitrepo
  $ cd gitrepo
  $ touch a
  $ git add a
  $ fn_git_commit -m a
  $ echo >> a
  $ fn_git_commit -am a2
  $ git log --oneline
  9da56a5 a2
  ad4fd0d a

  $ cd ..
  $ hg clone -q gitrepo hgrepo
  $ cd hgrepo
  $ hg log -G -T '{extras % "{extra}\n"}'
  @  branch=default
  |  convert_revision=9da56a563fafade1a5b50ae0c01292f91cd4ce34
  |  hg-git-rename-source=git
  o  branch=default
     convert_revision=ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9
     hg-git-rename-source=git

  $ cd ..
  $ hg clone -q hgrepo hgrepo2

Generate git-mapfile for a fresh repo
  $ cd hgrepo2
  $ test -f .hg/git-mapfile
  [1]
  $ hg git-updatemeta
  $ cat .hg/git-mapfile | sort
  9da56a563fafade1a5b50ae0c01292f91cd4ce34 f008e266042afb83012cda1e2cd65d108a51068f
  ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9 5b699970cd13b5f95f6af5f32781d80cfa2e813b

Add a new commit to git
  $ cd ../gitrepo
  $ echo >> a
  $ fn_git_commit -am a3
  $ git log --oneline -n 1
  1fc117f a3
  $ cd ../hgrepo
  $ hg pull -q

Update git-mapfile for a repo
  $ cd ../hgrepo2
  $ hg pull -q
  $ hg git-updatemeta
  $ cat .hg/git-mapfile | sort
  1fc117f64bf9ee3ae9b76e00d9cead51bce91e97 82e8585c3e4aa0dc511fc1c38c7382e4c728e58c
  9da56a563fafade1a5b50ae0c01292f91cd4ce34 f008e266042afb83012cda1e2cd65d108a51068f
  ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9 5b699970cd13b5f95f6af5f32781d80cfa2e813b

# Create a new commit, verify that git-updatemeta does not crash even though it
# does not have commit extras.
  $ cd ../hgrepo2
  $ touch c
  $ hg ci -Aqm c
  $ hg git-updatemeta
  $ cat .hg/git-mapfile | sort
  1fc117f64bf9ee3ae9b76e00d9cead51bce91e97 82e8585c3e4aa0dc511fc1c38c7382e4c728e58c
  9da56a563fafade1a5b50ae0c01292f91cd4ce34 f008e266042afb83012cda1e2cd65d108a51068f
  ad4fd0de4cb839a7d2d1c2497f8a2c230a2726e9 5b699970cd13b5f95f6af5f32781d80cfa2e813b
