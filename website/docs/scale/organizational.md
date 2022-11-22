---
sidebar_position: 30
---
# Organizational scale

Scale is often thought of as simply a technical matter: handle more data, and do it quickly.  Much less is said about the challenges organizational scale introduces on developer tools.  Over the past 10 years Sapling has encountered these challenges as the number of engineers has grown, as business needs have shifted, and as large repositories have been merged.

This document is a high-level overview of some of the strategies we’ve found useful.

### Scaling customer support

When you have thousands of engineers, every edge case will be hit and strange issues from strange environments will need to be debugged.  To stay on top of this, the Sapling team developed tools to make it easier to debug engineer issues quickly. Note, some of these are not available by default in the open source release since they require logging and services that only make sense within an organization.

* `sl doctor` is a command that attempts to fix a number of known issues, such as repository corruption. As new issues are found, `sl doctor` is updated to fix them, which allows engineers to self-remediate many issues, thereby helping reduce the support burden on the Sapling team.
* `sl rage` is a command that gathers state about the engineer’s source control environment and creates a human readable text file with the information.  It contains hostname, filesystem, disk usage, smartlog/status output, configuration, and recent commands, logs, errors, and profiles.  This information allows the engineer to provide us with all the data we need without having to have a bunch of back and forths.  The version of Sapling used inside Meta automatically uploads the rage file to a server for easy viewing by the Sapling team. The open source version does not upload any data.
* If an engineer complains about a slow command, adding `--profile` to any command allows them to produce a profile in the output. This makes it easy for the Sapling team to identify where the problem is, without having access to their machine or walking the engineer through how to use a profiler.  If a command is hung, a similar traceback is written to .sl/sigtraces/ every 60 seconds, allowing us to see exactly where the current process is stuck.  These tools make debugging tricky performance issues much easier, especially in a distributed environment.

### Managing configuration

Configuration is a critical part of the Sapling experience at Meta and plays a large part in how we roll out new features.  In the past we used a hierarchy of configuration files (similar to a `/etc/gitconfig` which recursively includes other config files) which were written by various systems (rpms, Chef, tools, etc).  This became unwieldy as the logic that decides which configs are used was distributed across many systems and services, and the order of precedence for the various config files was fragile.

To make configuration more scalable, we unified all configuration into an internal dynamic configuration system, cleverly called `dynamicconfig`.  This moved most configuration decisions from across the company into code in a single part of the code base where it could be reasoned about as a cohesive whole.  For configs that need to change frequently, like a hotfix remediation or a rollout, we are able to query an internal configuration server for a signed blob of configuration. This remote blob contains a simple JSON decision-tree DSL which the client then executes to determine which configs should be applied to this particular repository.

Both the in-code and the remote portions of the configuration allow conditionally enabling configs based on operating system, data center, user, repository, tier, machine shard, user shard, time shard, etc.  This allows us to rollout new features to particular machines or users, or to an automatically increasing percentage of users over time.  This flexibility has been key to letting Sapling move fast, as we can quickly release or rollback new features, and get feedback in an incremental fashion.

Note, this dynamic configuration system is internal to Meta and does not apply to the open source release. No remote configs are downloaded for an open source Sapling build.  The code for `dynamicconfig` is not currently visible in the repository as it contains internal configuration information. If external groups are interested in similar capabilities we could potentially make the capability public.

### Incremental migrations

Being able to make incremental breaking changes on an in-production system was a critical piece of making Sapling scale within Meta. Distributed source control repositories are particularly difficult structures to change because:

1. The contents are recursively hashed and the hashes are highly intertwined. Changing one byte changes every hash in the repository from there on.
2. The data is widely distributed so a migration has to happen on many machines and there is no single source of truth.

This makes incremental migrations hard, since changing anything in the repository affects all the hashes and since everyone’s individual copy of the repo will need to be migrated.

Within Meta, these migrations fell into two categories: repository merges and format migrations.

**Repository Merges**

Accomplishing repository merges involving thousands of people deserves a blog post of its own.  At a high level though, we developed server-side capabilities that let us “bind” directories in two repositories together so that any change to one was atomically applied to the other repository as well.  These bindings could be done either for the whole repository, or for individual directories in a repository.  This allowed us to make the contents of one repo available in another repo without any users having to immediately migrate. With the source control portion of the migration done transparently, the migration of users and tools could be done incrementally and in a way that was easy to rollback from.  Once all users and tools had migrated, we could shut down the old repository and remove the bindings.

**Format Migrations**

Changing the hash scheme of the repository or the on-disk storage formats was particularly tricky since it meant migrating every single repository on every single machine for every single engineer.  These migrations generally required writing to both the new and the old format for a while, often auditing that the results were equivalent, before turning off the old format.  This required strong metrics (see below) to guarantee that the change had rolled out widely enough and taken effect on almost all the machines. It also required strong guarantees about the ability to roll new versions of the Sapling package to all machines.  Internally we deploy new Sapling packages on a continuous basis, and prevent the execution of packages that are too old.

Hash scheme changes, while rare, required the additional step of maintaining a mapping between the new and the old hash scheme, and being able to validate past hashes which still used the old scheme.

### Metrics and logging

Having real-time metrics on source control commands is critical for maintaining and improving the source control experience within Meta. Internally, we upload a wide variety of metrics for every Sapling command that is run, from command duration, to time waiting on other services, to number of files changed, network throughput, error traces, and more. This lets us not only keep an eye on long-term performance and reliability trends, but enables us to see the impact of current rollouts on performance and catch regressions before they affect everyone.

The metrics are also useful for debugging individual developer issues.  If an engineer comes to us saying a given command failed yesterday, we’re able to look at the log and see exactly what Sapling commands ran around that time, where the commands spent their time, and how they exited.  By combining the logs with Commit Cloud, the internal service that backs up every commit as it is made, we’re often able to reproduce the exact situation the engineer experienced, without needing further information from the engineer.

Note, none of this applies to the open source release.  Neither metrics nor commits are uploaded from the open source Sapling builds. These logging and metrics tools are strictly internal to Meta.
