# Commit Cloud Scratch Bookmarks Sync

This process syncs scratch bookmarks from Mercurial into Mononoke. This works by
operating over a queue of changes that is kept up to date by Mercurial with new
entries whenever a scratch bookmark move happens (the `replaybookmarksqueue`
table in `xdb.infinitepush`).

The way the sync works is by pulling entries from the queue, consolidating them
by bookmarks, then attempting to update individual bookmarks. Since the entries
in the queue reference HG Changeset IDs (they were created in Mercurial), this
requires checking for a corresponding Bonsai Changeset first.

## Known Limitations

If too many bookmarks are repeatedly failing to sync (because their commits are
missing, forever), the process might stop making progress as it repeatedly
retries to apply those changes. Specifically, "too many" means "more than the
queue query limit".

This is probably acceptable for now, considering:

- There isn't that much volume on scratch bookmarks.
- The queue query limit is reasonably high compared to the expected volume of
  scratch bookmarks.

In addition, it's worth noting that if a bookmark is being updated concurrently
in two transactions in Mercurial, we might get an inconsistent ordering between
the replay queue and the state of bookmarks in Mercurial (similarly to what we
had happen in the Mononoke -> Mercurial sync job). For now, we're accepting this
risk, considering:

- Scratch bookmarks aren't getting that much traffic that this is likely to be a
  problem.
- Those two transactions would have to update the Bookmarks table in MySQL, and
  I believe MySQL will actually lock one transaction until the other completes
  in this case (compared to e.g. PostgreSQL, which will allow both to proceed
  then raise a serialization error later).
- Fixing this would require a substantial and risky rework of Mercurial's
  scratch bookmarks handling, which we're deprecating.


# Setup runbook

To enable live sync of all the scratch bookmarks to from HG to Mononoke please
use the following order of operations:

1. Start the queue population on HG servers by setting the `infinitepush.replaybookmarks` to `True`
2. Backfill the queue using `scripts/torozco/dump_bookmarkstonodes/`
3. Start bookmark filller with `--backfill` and wait for it to finish.
4. Start filller jobs without `--backfill`
