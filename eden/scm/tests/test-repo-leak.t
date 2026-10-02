
#require no-eden

#inprocess-hg-incompatible

Test native objects attached to the "repo" object gets properly released at the
end of process.

Attach an object with `__del__` to learn whether repo, ui are dropped on not.

  $ newext printondel <<EOF
  > class printondel(object):
  >     def __del__(self):
  >         print("__del__ called")
  > def reposetup(ui, repo):
  >     obj = printondel()
  >     repo._deltest = obj
  >     ui._deltest = obj
  > EOF

  $ configure modern

No leak without extensions

  $ newclientrepo >/dev/null

  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: blackbox

  $ newclientrepo >/dev/null
  $ setconfig extensions.blackbox=
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: remotefilelog

  $ newclientrepo >/dev/null
  $ echo remotefilelog >> .sl/requires
  $ setconfig extensions.remotefilelog= remotefilelog.cachepath=$TESTTMP/cache
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: treemanifest

  $ newclientrepo >/dev/null
  $ setconfig remotefilelog.reponame=x
  $ sl log -r . -T '{node}\n'
  0000000000000000000000000000000000000000
  __del__ called
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: treemanifest only

  $ newclientrepo >/dev/null
  $ setconfig remotefilelog.reponame=x
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: sparse

  $ newclientrepo >/dev/null
  $ setconfig extensions.sparse=
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: commitcloud

  $ newclientrepo >/dev/null
  $ setconfig extensions.commitcloud=
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Fine extension: sampling

  $ newclientrepo >/dev/null
  $ setconfig extensions.sampling=
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

Somehow problematic: With many extensions

  $ newclientrepo >/dev/null
  $ echo remotefilelog >> .sl/requires
  $ cat >> .sl/config <<EOF
  > [extensions]
  > absorb=
  > amend=
  > arcdiff=
  > automv=
  > blackbox=
  > chistedit=
  > cleanobsstore=!
  > clienttelemetry=
  > clindex=
  > color=
  > commitcloud=
  > conflictinfo=
  > crdump=
  > debugcommitmessage=
  > dialect=
  > directaccess=
  > dirsync=
  > errorredirect=!
  > extorder=
  > extorder=
  > fastlog=
  > fastpartialmatch=!
  > fbcodereview=
  > fbhistedit=
  > githelp=
  > gitlookup=!
  > gitrevset=!
  > grpcheck=
  > hgevents=
  > histedit=
  > journal=
  > lz4revlog=
  > mergedriver =
  > mergedriver=
  > morestatus=
  > myparent=
  > phrevset=
  > progressfile=
  > pushrebase =
  > pushrebase=
  > rage=
  > rebase =
  > rebase=
  > reset=
  > sampling=
  > shelve=
  > sigtrace=
  > smartlog=
  > sparse=
  > stat=
  > traceprof=
  > treedirstate=
  > tweakdefaults=
  > undo=
  > 
  > [phases]
  > publish = False
  > 
  > [remotefilelog]
  > reponame = x
  > cachepath = $TESTTMP/cache
  > 
  > [fbscmquery]
  > host=example.com
  > path=/conduit/
  > reponame=x
  > EOF
  $ sl log -r . -T '{manifest % "{node}"}\n'
  0000000000000000000000000000000000000000
  __del__ called

  $ touch x

FIXME: this is problematic in non-buck build.
 (this behaves differently with buck / setup.py build)

  $ sl ci -m x -A x
  __del__ called (?)

  $ sl log -r . -T '{manifest % "{node}"}\n'
  c2ffc254676c538a75532e7b6ebbbccaf98e2545
  __del__ called
