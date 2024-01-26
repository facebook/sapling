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
