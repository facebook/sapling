# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# NOTE: this test exercises the dry-run work-list path only. The integration
# harness creates a single, non-deep-sharded test repo, so it CANNOT reproduce
# the deep-sharded case (where the reconciler must resolve each repo's config via
# load_all_repo_configs()/on-demand loading rather than the eager
# app.repo_configs().repos map, which omits deep-sharded repos). That fix lives in
# backfill_reconcile_configs.rs and must be validated in prod (see diff summary).
# --apply is never exercised here (it would hit real configerator).

setup a repo whose enabled derived-data config contains only fsnodes, so we
have a clean in-config type (fsnodes) and an out-of-config type (unodes)
  $ ENABLED_DERIVED_DATA="fsnodes" setup_common_config

empty case: nothing enabled in the table yet -> nothing to reconcile
  $ mononoke_admin derived-data backfill-reconcile-configs
  Scanned 0 enablement row(s): 0 pending, 0 already in config, 0 repo(s) not in loaded configs.
  Nothing to reconcile: all enabled types are already present in config.

mark a type that is NOT already in the repo config (unodes is not in ENABLED_DERIVED_DATA)
  $ mononoke_admin derived-data enabled-types set -R repo -T unodes --i-know-what-i-am-doing

dry-run lists the pending (repo, type); no --apply so nothing is landed
  $ mononoke_admin derived-data backfill-reconcile-configs
  Scanned 1 enablement row(s): 1 pending, 0 already in config, 0 repo(s) not in loaded configs.
  Reconciliation plan: 1 pending (repo, type) enablement(s) across 1 batch(es):
  Batch 1 (1 repos):
    repo_id=0 repo_name=repo type=unodes config=default
  
  Dry run: no configerator changes were made. Re-run with --apply to create review diff(s).


mark a type that IS already in the repo config (fsnodes is in ENABLED_DERIVED_DATA)
  $ mononoke_admin derived-data enabled-types set -R repo -T fsnodes --i-know-what-i-am-doing

fsnodes is already reconciled (present in config), so only unodes remains pending
  $ mononoke_admin derived-data backfill-reconcile-configs
  Scanned 2 enablement row(s): 1 pending, 1 already in config, 0 repo(s) not in loaded configs.
  Reconciliation plan: 1 pending (repo, type) enablement(s) across 1 batch(es):
  Batch 1 (1 repos):
    repo_id=0 repo_name=repo type=unodes config=default
  
  Dry run: no configerator changes were made. Re-run with --apply to create review diff(s).


unset the pending type; the table now only holds already-in-config fsnodes
  $ mononoke_admin derived-data enabled-types unset -R repo -T unodes --i-know-what-i-am-doing

back to nothing to reconcile (fsnodes already in config, unodes removed)
  $ mononoke_admin derived-data backfill-reconcile-configs
  Scanned 1 enablement row(s): 0 pending, 1 already in config, 0 repo(s) not in loaded configs.
  Nothing to reconcile: all enabled types are already present in config.
