/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <fmt/format.h>

#include "eden/common/utils/benchharness/Bench.h"
#include "eden/fs/journal/Journal.h"

using namespace facebook::eden;

namespace {

constexpr size_t kJournalEntries = 100'000;

void populateJournal(Journal& journal, bool metadataOnly) {
  for (size_t i = 0; i < kJournalEntries; ++i) {
    // Distinct paths so consecutive deltas are not merged.
    auto path = metadataOnly ? fmt::format(".hg/blackbox.log.{}", i)
                             : fmt::format("foo/bar/baz.{}", i);
    journal.recordChanged(RelativePath{path}, dtype_t::Regular);
  }
}

/**
 * The validity check ScmStatusCache performs on the getScmStatusV2 hot path:
 * only three booleans of the accumulated range are read.
 */
bool validityViaAccumulateRange(Journal& journal) {
  auto range = journal.accumulateRange(1);
  if (!range) {
    return false;
  }
  return !range->isTruncated && range->containsHgOnlyChanges &&
      !range->containsRootUpdate;
}

void accumulate_range_validity_repo_metadata_only(benchmark::State& state) {
  Journal journal{makeRefPtr<EdenStats>()};
  populateJournal(journal, /*metadataOnly=*/true);
  for (auto _ : state) {
    benchmark::DoNotOptimize(validityViaAccumulateRange(journal));
  }
}

void accumulate_range_validity_working_copy_only(benchmark::State& state) {
  Journal journal{makeRefPtr<EdenStats>()};
  populateJournal(journal, /*metadataOnly=*/false);
  for (auto _ : state) {
    benchmark::DoNotOptimize(validityViaAccumulateRange(journal));
  }
}

BENCHMARK(accumulate_range_validity_repo_metadata_only);
BENCHMARK(accumulate_range_validity_working_copy_only);

} // namespace

EDEN_BENCHMARK_MAIN();
