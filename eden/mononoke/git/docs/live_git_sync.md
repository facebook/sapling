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

Note: You will want to set-up all the configuration in one diff, but don't land it until you are done preparing the Mononoke repo.
In the meantime, you will want to communicate about your diff, in particular to avoid any conflicts with the assignment of a `repo_id`.

#### Add the configuration for the repo itself

* In [git.cinc](https://www.internalfb.com/code/configerator/[master]/source/scm/mononoke/repos/repos/git.cinc), define the configuration for the Mononoke repo where the git repo will be mirrored

You may take inspiration from existing repos in there.

Note:
Please, disable derivation for `hgchangesets` and `filenodes`: these are not compatible with submodules and we want to set a consistent expectation for users of live synced git repos: Hg changesets are not available for such repositories.

#### Add it to the repo definitions

* In [repo_definitions.cconf](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/repo_definitions.cconf), add the repo to the [`repo_definitions` dictionary](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/repos/repo_definitions.cconf?lines=17)

A few notes:
* The key is the Mononoke repo name (e.g `"watsapp/server"`)
* The `repo_config` value is the variable name as defined in `git.cinc` in the previous step
* The `repo_id` value must never have been used in this file. That includes in diffs that weren't pushed to master, but were canaried... (This should be improved)
* The `external_repo_id` value is currently unused but we try to keep it in sync with the repo_id in `xdb.metagit` (`select id,name from repositories where name like 'git/repo/name';` will tell you)

#### Configure the repo as part of the `scs` tier

If a git repo contains or has ever contained submodules, we can't currently derive `hgchangesets` or `filenodes` for its Bonsai commits as submodules are not currently supported by sapling at all and derivation would fail.
For that reason, any such repository cannot be part of the `prod` tier and has to be part of the `scs` tier. To provide a consistent experience to git repo users, we decide to only set new gitrepos as members of the scs tier, and with `hgchangesets` and `filenodes` derivation disabled.

* Make the repo part of the [`scs` tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/scs.cconf)
    * Add it to [scs_repo_definitions](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=91) and [scs_repo_configs](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=98)

#### Configure the repo as part of the `gitimport_content` tier

* Add the repo to the [`gitimport_content` tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/gitimport_content.cconf)
   * [remote_gitimport_repo_definitions](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=106) and [remote_gitimport_repo_configs](https://www.internalfb.com/code/configerator/[3ae88dc5438d]/source/scm/mononoke/repos/repos/prod.cinc?lines=111) are necessary to make the mirror repos part of the [gitimport_content tier](https://www.internalfb.com/code/configerator/[3ae88dc5438d1ca6846708d7381a2b9aa057d157]/source/scm/mononoke/repos/tiers/gitimport_content.cconf)

#### Configure sharding for the repo

* Create a [`RawRepoShardingConfig`](https://www.internalfb.com/code/configerator/[e3b67b723b7e572503d18792df72d8bb45c49249]/source/scm/mononoke/sharding/sharding.cinc?lines=789), configuring derived data tailer, scs and derivation worker for the repo.
* Do not configure Eden API for the repo as it doesn't support `hg changesets`

* Add that config to the [`sharded_repo-configs` dict](https://www.internalfb.com/code/configerator/[e3b67b723b7e572503d18792df72d8bb45c49249]/source/scm/mononoke/sharding/sharding.cinc?lines=825)

#### Exclude the new repo from the streaming changelog alerting

* Add the repo to [`repos_to_skip`](https://www.internalfb.com/code/configerator/[e82b3da4be730cb097524ee41f3da8657f4c12a5]/source/scm/mononoke/detectors/streaming_changelog_builder.detector.cconf?lines=35) for the streaming changelog builder detector (as we're not setting up streaming changelog for this repo) 

#### Setup the ACLs

* [`gitremoteimport` service identity](https://www.internalfb.com/amp/identity?type=SERVICE_IDENTITY&name=gitremoteimport) must be granted read access to the git repository. e.g. [whatsapp repos](https://www.internalfb.com/amp/ACL/REPO%3Arepos%2Fgit%2Fwhatsapp) or [aosp repos](https://www.internalfb.com/amp/ACL/REPO%3Arepos%2Fgit%2Faosp)

#### Configure the live sync hooks

* Configure the [hooks](https://www.internalfb.com/code/configerator/[130cdab06b41f531aca5b325b05913770c485ab2]/source/scm/repoconfigs/gitrepoconfigs.cconf?lines=76) for this git repo to contain the live sync hooks:
```
        default_and_hooks(
            HookRunlists(
                update={
                    "999-upload-to-mononoke.sh",
                },
                post_receive={
                    "999-update-mononoke-bookmarks.sh",
                },
            )
        ),
```

### Perform the initial import and backfilling

* Clone the git repo locally (for instance, under `~/gitrepos`)
```
mkdir ~/gitrepos
cd ~/gitrepos
git clone ssh://git.vip.facebook.com//data/gitrepos/<git-repo-name.git>
```

* Having configured the Mononoke repo (see above), raise a diff for awareness. Do not land it until a bit later once everything is backfilled, but it is crucial that other team members are aware of the `repo_id` you are using so they don't re-use it by mistake.

* Canary the diff locally
```
arc canary
```

Note: In the context of live git sync, don't ever use the `--discard-submodules` flag of `gitimport`. That would cause the import to be lossy.
* Import all commits and all bookmarks for the repository
```
buck2 run @//mode/opt //eden/mononoke/git/gitimport:gitimport -- --local-configerator-path ~/configerator/materialized_configs --config-tier=scs.materialized_JSON --repo-name <mononoke-repo-name> ~/gitrepos/<git-repo-name> --generate-bookmarks full-repo
```

If the import fails for any reason (for instance, missing some ACLs config), please take the time to update the instructions in this wiki to fill-in the gaps for the next person.

* Backfill all derived data types for the repo
In the previous step, you have imported all commits and all bookmarks, which means that all imported commits are public.
Backfill all derived data for all public commits (see [instructions here](https://www.internalfb.com/intern/wiki/Source_Control/Mononoke/Development/Backfilling_Derived_Data/))

### Enable live sync

#### Pre-land
In the test plan for your diff, we recommend:
* Waiting for one push to master
* Canarying the configerator diff for `gitimport_content`
* Running `remote_gitimport` locally to catch-up the commits import
* Running `git_move_bookmark` locally to catch-up the bookmark
If this process succeeds, you may be relatively confident that the live sync will work correctly.

#### Land
* Post in Source Control Rollouts
* Land your configerator diff

#### Post-Land
* Monitor the following tables to make sure that pushes are being correctly live-synced:
    * [`metagit_cli`](https://fburl.com/scuba/metagit_cli/tlaia50r): Filtering for pushes to the repo in question, you can see if pushes are failing. Failing pushes should cause alerts, but also be vigilant to pushes hanging
    * [`mononoke_remote_gitimport_bookmarks`](https://fburl.com/scuba/mononoke_gitimport_content/a1mxzyc9): Any failure here means that the hook that syncs bookmarks is failing
    * [`mononoke_remote_gitimport_commits`](https://fburl.com/scuba/mononoke_remote_gitimport_commits/ix13ace9): Any failure here means that the hook that imports commits is failing
    * [`mononoke_gitimport_content`](https://fburl.com/scuba/mononoke_gitimport_content/a1mxzyc9): Any failure in here means that the gitimport_content task is failing. That should cause commits sync to fail
