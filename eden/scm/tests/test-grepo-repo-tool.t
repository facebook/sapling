#require git grepo grepo-source no-windows no-eden symlink

This test runs the `repo` tool installed on the host. `repo init` clones the
tool source from a local clone. So the test uses no network and no host
credentials. To run the test, clone the tool source from the default
`REPO_URL` of the launcher. Then set HGTEST_GREPO_SOURCE_URL to the path of
the clone. If HGTEST_GREPO_SOURCE_URL is not set, the test is skipped.

  $ . $TESTDIR/git.sh
  $ enable smartlog
  $ setconfig grepo.manifestpath=static/static.xml

Set up two project repos and a manifest repo for `repo init` to fetch:

  $ git init -q -b main project-a
  $ cd project-a
  $ echo "project-a content" > README
  $ git add README && git commit -qm 'init a'
  $ A_REV=$(git rev-parse HEAD)
  $ echo "project-a content v2" > README
  $ git add README && git commit -qm 'update a'
  $ A2_REV=$(git rev-parse HEAD)
  $ cd ..

  $ git init -q -b main project-b
  $ cd project-b
  $ echo "project-b content" > README
  $ git add README && git commit -qm 'init b'
  $ B_REV=$(git rev-parse HEAD)
  $ echo "project-b content v2" > README
  $ git add README && git commit -qm 'update b'
  $ B2_REV=$(git rev-parse HEAD)
  $ cd ..

  $ git init -q -b main manifest
  $ mkdir manifest/static
  $ writemanifest() {
  >   cat > manifest/static/static.xml << EOF
  > <?xml version="1.0" encoding="UTF-8"?>
  > <manifest>
  >   <remote name="origin" fetch="file://$TESTTMP"/>
  >   <default revision="main" remote="origin"/>
  >   <project name="project-a" path="vendor/a" revision="$1"/>
  >   <project name="project-b" path="frameworks/b" revision="$2"/>
  > </manifest>
  > EOF
  >   git -C manifest add static/static.xml && git -C manifest commit -qm "$3"
  > }
  $ writemanifest $A_REV $B_REV 'add manifest'
  $ writemanifest $A2_REV $B_REV 'bump vendor/a'
  $ writemanifest $A2_REV $B2_REV 'bump frameworks/b'

Create the workspace with the real repo tool:

  >>> from sapling.testing.t import hghave
  >>> setenv("GREPO_SOURCE_URL", hghave.grepo_source_url)
  $ mkdir repodir && cd repodir
  $ repo init -q -u file://$TESTTMP/manifest -m static/static.xml -b main --repo-url=$GREPO_SOURCE_URL > /dev/null 2>&1
  $ repo sync -q -j4
  $ touch .repo/enable_sl

`repo init` puts the manifests repo on the local branch `default`. `repo sync`
adds the remote refs `origin/main` and `m/main`:

  $ git -C .repo/manifests symbolic-ref HEAD
  refs/heads/default
  $ git -C .repo/manifests config branch.default.merge
  refs/heads/main
  $ git -C .repo/manifests for-each-ref --format='%(refname)'
  refs/heads/default
  refs/remotes/m/main
  refs/remotes/origin/main

  $ sl root
  $TESTTMP/repodir
  $ sl status

All synced manifest commits are reachable from a remote ref, so they are
public. Smartlog (and ISL) do not show public commits without a name:

  $ sl log -r 'all()' -T '{desc}: {phase} {bookmarks} {remotenames}\n'
  add manifest: public  
  bump vendor/a: public  
  bump frameworks/b: public default m/main origin/main
  $ sl smartlog -T '{desc} {bookmarks} {remotenames}'
  @  bump frameworks/b default m/main origin/main
  │
  ~

Go back to the first manifest commit. `default` follows the working copy
parent. The other commits stay public and reachable from `origin/main`:

  $ sl goto -q 'desc("add manifest")'
  $ sl log -r 'all()' -T '{desc}: {phase} {bookmarks} {remotenames}\n'
  add manifest: public default 
  bump vendor/a: public  
  bump frameworks/b: public  m/main origin/main
  $ sl smartlog -T '{desc} {bookmarks} {remotenames}'
  o  bump frameworks/b  m/main origin/main
  ╷
  @  add manifest default

A local commit in the manifests repo is draft. `repo` adds a `Change-Id`
trailer to it:

  $ sl goto -q 'desc("bump frameworks/b")'
  $ echo local > .repo/manifests/local.txt
  $ git -C .repo/manifests add local.txt
  $ git -C .repo/manifests commit -qm 'local manifest change'
  $ sl log -r 'all()' -T '{desc|firstline}: {phase} {bookmarks} {remotenames}\n'
  add manifest: public  
  bump vendor/a: public  
  bump frameworks/b: public  m/main origin/main
  local manifest change: draft default 
  $ sl smartlog -T '{desc|firstline} {bookmarks} {remotenames}'
  @  local manifest change default
  │
  o  bump frameworks/b  m/main origin/main
  │
  ~

Go back to the first manifest commit again. This is bad: `default` moves away
from the draft commit. No ref points to the draft commit, so it disappears:

  $ sl goto -q 'desc("add manifest")'
  $ sl log -r 'all()' -T '{desc|firstline}: {phase} {bookmarks} {remotenames}\n'
  add manifest: public default 
  bump vendor/a: public  
  bump frameworks/b: public  m/main origin/main
  $ sl smartlog -T '{desc|firstline} {bookmarks} {remotenames}'
  o  bump frameworks/b  m/main origin/main
  ╷
  @  add manifest default
