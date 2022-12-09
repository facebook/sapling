#testcases normalcheckout nativecheckout
  $ configure modernclient
  $ enable sparse

#if nativecheckout
  $ setconfig experimental.nativecheckout=True
#endif

  $ newclientrepo
  $ touch init
  $ hg commit -Aqm initial
  $ touch alwaysincluded
  $ touch excludedbyconfig
  $ touch includedbyconfig
  $ cat >> sparseprofile <<EOF
  > [include]
  > alwaysincluded
  > excludedbyconfig
  > EOF
  $ hg commit -Aqm 'add files'
  $ echo >> alwaysincluded
  $ hg commit -Aqm 'modify'
  $ hg sparse enable sparseprofile
  $ ls
  alwaysincluded
  excludedbyconfig

Test hg goto reads new hgrc profile config
  $ cp .hg/hgrc .hg/hgrc.bak
  $ cat >> .hg/hgrc <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
# Run a no-op command to verify it does not refresh the sparse profile with the
# new config.
  $ hg log -r . -T '{desc}\n'
  modify

  $ ls
  alwaysincluded
  excludedbyconfig

# hg up should update to use the new config
  $ hg up -q .^
  $ ls
  alwaysincluded
  includedbyconfig

  $ cat .hg/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Test hg updating back to original location keeps the new hgrc profile config
  $ hg up -q tip
  $ ls
  alwaysincluded
  includedbyconfig

  $ cat .hg/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Reset to remove hgrc profile config
  $ cp .hg/hgrc.bak .hg/hgrc
  $ hg up -q .^
  $ hg up -q .~-1
  $ ls
  alwaysincluded
  excludedbyconfig

  $ cat .hg/sparseprofileconfigs
  {} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Test hg commit does not read new hgrc profile config
  $ cat >> .hg/hgrc <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
  $ echo >> alwaysincluded
  $ hg commit -m 'modify alwaysincluded'
  $ ls
  alwaysincluded
  excludedbyconfig

  $ cat .hg/sparseprofileconfigs
  {} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Update to get latest config
  $ hg up -q .^
  $ hg up -q .~-1
  $ cat .hg/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Reset
  $ cp .hg/hgrc.bak .hg/hgrc
  $ hg up -q .^
  $ hg up -q .~-1

Cleanly crash an update and verify the new config was not applied
  $ cat > ../killer.py << EOF
  > from edenscm import error, extensions, localrepo
  > def setparents(orig, repo, *args, **kwargs):
  >     raise error.Abort("bad thing happened")
  > 
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, "setparents",
  >                             setparents)
  > EOF

  $ cat >> .hg/hgrc <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
  $ hg up .^ --config extensions.killer=$TESTTMP/killer.py
  abort: bad thing happened
  [255]
  $ cat .hg/sparseprofileconfigs
  {} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

But a successful update does get the new config
  $ hg up -q .^
  $ cat .hg/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs

Reset
  $ cp .hg/hgrc.bak .hg/hgrc
  $ hg up -q .^
  $ hg up -q .~-1

Hard killing the process leaves the pending config file around
  $ cat >> .hg/hgrc <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF

  $ cat > ../killer.py << EOF
  > import os
  > from edenscm import extensions, localrepo
  > def setparents(orig, repo, *args, **kwargs):
  >     # os._exit skips all cleanup
  >     os._exit(100)
  > 
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, "setparents",
  >                             setparents)
  > EOF
  $ hg up .^ --config extensions.killer=$TESTTMP/killer.py
  [100]
  $ cat .hg/sparseprofileconfigs
  {} (no-eol)
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs
  .hg/sparseprofileconfigs.* (glob)

But it is not consumed (alwaysincluded should show up in the list)
  $ hg files
  alwaysincluded
  excludedbyconfig

And is cleaned up on the next update
  $ hg up -q .^
  $ ls .hg/sparseprofileconfigs*
  .hg/sparseprofileconfigs
  $ hg files
  alwaysincluded
  includedbyconfig

Reset
  $ cp .hg/hgrc.bak .hg/hgrc
  $ hg up -q .~-1
