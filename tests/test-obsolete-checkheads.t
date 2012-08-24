Check that obsolete properly strip heads
  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ cat >> $HGRCPATH << EOF
  > [phases]
  > # public changeset are not obsolete
  > publish=false
  > [ui]
  > logtemplate='{node|short} ({phase}) {desc|firstline}\n'
  > [extensions]
  > graphlog=
  > EOF
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH
  $ mkcommit() {
  >    echo "$1" > "$1"
  >    hg add "$1"
  >    hg ci -m "add $1"
  > }
  $ getid() {
  >    hg id --debug -ir "desc('$1')"
  > }


  $ hg init remote
  $ cd remote
  $ mkcommit base
  $ hg phase --public .
  $ cd ..
  $ cp -r remote base
  $ hg clone remote local
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd local

New head replaces old head
==========================

setup

  $ mkcommit old
  $ hg push
  pushing to $TESTTMP/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg up -q '.^'
  $ mkcommit new
  created new head
  $ hg debugobsolete `getid old` `getid new`
  $ hg glog --hidden
  @  71e3228bffe1 (draft) add new
  |
  | x  c70b08862e08 (draft) add old
  |/
  o  b4952fcf48cf (public) add base
  
  $ cp -r ../remote ../backup1

old exists remotely as draft. It is obsoleted by new that we now push.
Push should not warn about creating new head

  $ hg push
  pushing to $TESTTMP/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)

old head is now public (public local version)
=============================================

setup

  $ rm -fr ../remote
  $ cp -r ../backup1 ../remote
  $ hg -R ../remote phase --public c70b08862e08
  $ hg pull -v
  pulling from $TESTTMP/remote
  searching for changes
  no changes found
  $ hg glog --hidden
  @  71e3228bffe1 (draft) add new
  |
  | o  c70b08862e08 (public) add old
  |/
  o  b4952fcf48cf (public) add base
  

Abort: old will still be an head because it's public.

  $ hg push
  pushing to $TESTTMP/remote
  searching for changes
  abort: push creates new remote head 71e3228bffe1!
  (did you forget to merge? use push -f to force)
  [255]

old head is now public (public remote version)
==============================================

TODO: Not implemented yet.

# setup
#
#   $ rm -fr ../remote
#   $ cp -r ../backup1 ../remote
#   $ hg -R ../remote phase --public c70b08862e08
#   $ hg phase --draft --force c70b08862e08
#   $ hg glog --hidden
#   @  71e3228bffe1 (draft) add new
#   |
#   | x  c70b08862e08 (draft) add old
#   |/
#   o  b4952fcf48cf (public) add base
#
#
#
# Abort: old will still be an head because it's public.
#
#   $ hg push
#   pushing to $TESTTMP/remote
#   searching for changes
#   abort: push creates new remote head 71e3228bffe1!
#   (did you forget to merge? use push -f to force)
#   [255]

old head is obsolete but replacement is not pushed
==================================================

setup

  $ rm -fr ../remote
  $ cp -r ../backup1 ../remote
  $ hg phase --draft --force '(0::) - 0'
  $ hg up -q '.^'
  $ mkcommit other
  created new head
  $ hg glog --hidden
  @  d7d41ccbd4de (draft) add other
  |
  | o  71e3228bffe1 (draft) add new
  |/
  | x  c70b08862e08 (draft) add old
  |/
  o  b4952fcf48cf (public) add base
  

old exists remotely as draft. It is obsoleted by new but we don't push new.
Push should abort on new head

  $ hg push -r 'desc("other")'
  pushing to $TESTTMP/remote
  searching for changes
  abort: push creates new remote head d7d41ccbd4de!
  (did you forget to merge? use push -f to force)
  [255]
