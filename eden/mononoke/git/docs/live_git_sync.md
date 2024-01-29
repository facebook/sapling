# Live git sync

The process of importing git repositories into Mononoke and keeping the live-sync running is an area of active development for the Mononoke team.

Here, we are trying to give an accurate picture of what's involved at the moment to import an existing repository into Mononoke and set-up live sync for it.

## Scope

By live-sync, we mean:
* A Mononoke repository is set-up to mirror a git repository in metagit.
* The Mononoke repository has one bonsai commit for each git commit in the git repository.
    * The bonsai commits are a superset of the git commits, so that each git commit could be re-created from the information stored in Mononoke only.
* The Mononoke repository has one bookmark for each branch or tag in the git repository.
* As pushes happen to the Git repository, the Mononoke repository stays in-sync.

Sometimes, we may want to mirror a repo coming from git into another Mononoke repo (like [whatsapp/server to fbsource here](https://fb.workplace.com/groups/sourcecontrolteam/permalink/6855936641194290/)).
That is cross-repo sync and is orthogonal to live git sync. It is not covered here.

## Process overview

Consider an existing repository, where we want to setup live sync.
It is roughly a 3 steps process:

* Configure the new Mononoke repository
* Perform the initial import and backfilling
* Set-up ongoing sync

### Configure the new Mononoke repository

#### Add the configuration for the repo itself

* In [git.cinc](https://www.internalfb.com/code/configerator/[master]/source/scm/mononoke/repos/repos/git.cinc), define the configuration for the Mononoke repo where the git repo will be mirrored

You may take inspiration from existing repos in there.

Note:
You must disable derivation for `hgchangesets` and `filenodes` if the repo you are importing contains or has ever contained submodules. That is because sapling does not currently support submodules, so derivation would fail for any commit containing submodules.

#### Add it to the repo definitions

* In [repo_definitions.cconf](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/repo_definitions.cconf), add the repo to the [`repo_definitions` dictionary](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/repo_definitions.cconf?lines=17)

A few notes:
* The key is the Mononoke repo name (e.g `"watsapp/server"`)
* The `repo_config` value is the variable name as defined in `git.cinc` in the previous step
* The `repo_id` value must never have been used in this file. That includes in diffs that weren't pushed to master, but were canaried... (This should be improved)
* The `external_repo_id` value is currently unused but we try to keep it in sync with the repo_id in `xdb.metagit` (`select id,name from repositories where name like 'git/repo/name';` will tell you)

#### Configure the repo as part of the `scs` or `prod` tier

If a git repo contains or has ever contained submodules, we can't currently derive `hgchangesets` or `filenodes` for its Bonsai commits as submodules are not currently supported by sapling at all and derivation would fail.
For that reason, any such repository cannot be part of the `prod` tier and has to be part of the `scs` tier.

If a git repo does not contain anything that makes it impossible to derive hgchangesets for it, one can choose to set-it up as a `prod` repo.

* Make the repo part of the [`scs` tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/scs.cconf)
    * Add it to [scs_repo_definitions](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=91) and [scs_repo_configs](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=98)
**or**
* Make the repo part of the [`prod` tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/prod.cconf)
    * Add it to [prod_repo_configs](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/prod.cinc?lines=11) and [prod_repo_definitions](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/prod.cinc?lines=49)

#### Setup the ACLs

TODO: document which ACLs are needed

### Perform the initial import and backfilling

TODO: document `gitimport` + link to derived data backfilling

### Set-up ongoing sync

* Add the repo to the [`gitimport_content` tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/gitimport_content.cconf)
   * [remote_gitimport_repo_definitions](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=106) and [remote_gitimport_repo_configs](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=111) are necessary to make the mirror repos part of the [gitimport_content tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/gitimport_content.cconf)

TODO: document hooks configuration
TODO: document remote_gitimport
TODO: document git_move_bookmarks
