#testcases normalcheckout nativecheckout
#chg-compatible
#inprocess-hg-incompatible
#require no-eden
  $ configure modernclient
  $ enable sparse
  $ setconfig commands.update.check=none

#if nativecheckout
  $ setconfig experimental.nativecheckout=True
#endif

  $ newclientrepo
  $ touch init
  $ sl commit -Aqm initial
  $ touch alwaysincluded
  $ touch excludedbyconfig
  $ touch includedbyconfig
  $ cat >> sparseprofile <<EOF
  > [include]
  > alwaysincluded
  > excludedbyconfig
  > EOF
  $ sl commit -Aqm 'add files'
  $ echo >> alwaysincluded
  $ sl commit -Aqm 'modify'
  $ sl sparse enable sparseprofile
  $ ls
  alwaysincluded
  excludedbyconfig

Test sl goto reads new hgrc profile config
  $ cp .sl/config .sl/config.bak
  $ cat >> .sl/config <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
# Run a no-op command to verify it does not refresh the sparse profile with the
# new config.
  $ sl log -r . -T '{desc}\n'
  modify

  $ ls
  alwaysincluded
  excludedbyconfig

# sl up should update to use the new config
  $ sl up -q .^
  $ ls
  alwaysincluded
  includedbyconfig

  $ cat .sl/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Test sl updating back to original location keeps the new hgrc profile config
  $ sl up -q tip
  $ ls
  alwaysincluded
  includedbyconfig

  $ cat .sl/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Reset to remove hgrc profile config
  $ cp .sl/config.bak .sl/config
  $ sl up -q .^
  $ sl up -q .~-1
  $ ls
  alwaysincluded
  excludedbyconfig

  $ cat .sl/sparseprofileconfigs
  {} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Test sl commit does not read new hgrc profile config
  $ cat >> .sl/config <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
  $ echo >> alwaysincluded
  $ sl commit -m 'modify alwaysincluded'
  $ ls
  alwaysincluded
  excludedbyconfig

  $ cat .sl/sparseprofileconfigs
  {} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Update to get latest config
  $ sl up -q .^
  $ sl up -q .~-1
  $ cat .sl/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Reset
  $ cp .sl/config.bak .sl/config
  $ sl up -q .^
  $ sl up -q .~-1

Cleanly crash an update and verify the new config was not applied
  $ cat > ../killer.py << EOF
  > from sapling import error, extensions, localrepo
  > def setparents(orig, repo, *args, **kwargs):
  >     raise error.Abort("bad thing happened")
  > 
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, "setparents",
  >                             setparents)
  > EOF

  $ cat >> .sl/config <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF
  $ sl up .^ --config extensions.killer=$TESTTMP/killer.py
  abort: bad thing happened
  [255]
  $ cat .sl/sparseprofileconfigs
  {} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

But a successful update does get the new config
  $ sl up -q .^
  $ cat .sl/sparseprofileconfigs
  {"sparseprofile": "[include]\nincludedbyconfig\n[exclude]\nexcludedbyconfig\n"} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs

Reset
  $ cp .sl/config.bak .sl/config
  $ sl up -q .^
  $ sl up -q .~-1

Hard killing the process leaves the pending config file around
  $ cat >> .sl/config <<EOF
  > [sparseprofile]
  > include.foo.sparseprofile=includedbyconfig
  > exclude.bar.sparseprofile=excludedbyconfig
  > EOF

  $ cat > ../killer.py << EOF
  > import os
  > from sapling import extensions, localrepo
  > def setparents(orig, repo, *args, **kwargs):
  >     # os._exit skips all cleanup
  >     os._exit(255)
  > 
  > def extsetup(ui):
  >     extensions.wrapfunction(localrepo.localrepository, "setparents",
  >                             setparents)
  > EOF
  $ sl up .^ --config extensions.killer=$TESTTMP/killer.py
  [255]
  $ cat .sl/sparseprofileconfigs
  {} (no-eol)
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs
  .sl/sparseprofileconfigs.* (glob)

But it is not consumed (alwaysincluded should show up in the list)
  $ sl files
  alwaysincluded
  excludedbyconfig

And is cleaned up on the next update
  $ sl up -q .^
  $ ls .sl/sparseprofileconfigs*
  .sl/sparseprofileconfigs
  $ sl files
  alwaysincluded
  includedbyconfig

Empty (truncated after power cycle) sparseprofileconfigs does not break things

  $ echo 'x' > .sl/sparseprofileconfigs
  $ sl status
  $ sl files
  alwaysincluded

Reset
  $ cp .sl/config.bak .sl/config
  $ sl up -q .~-1
