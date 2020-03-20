/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/logging/xlog.h>
#include <gtest/gtest.h>
#include <array>
#include <memory>

#include "eden/fs/model/Hash.h"
#include "eden/fs/store/ImportPriority.h"
#include "eden/fs/store/hg/HgImportRequest.h"
#include "eden/fs/store/hg/HgImportRequestQueue.h"
#include "eden/fs/utils/IDGen.h"

using namespace facebook::eden;

Hash uniqueHash() {
  std::array<uint8_t, Hash::RAW_SIZE> bytes = {0};
  auto uid = generateUniqueID();
  std::memcpy(bytes.data(), &uid, sizeof(uid));
  return Hash{bytes};
}

std::pair<Hash, HgImportRequest> makeImportRequest(ImportPriority priority) {
  auto hash = uniqueHash();
  return std::make_pair(
      hash, HgImportRequest::makeBlobImportRequest(hash, priority).first);
}

TEST(HgImportRequestQueueTest, getRequestByPriority) {
  auto queue = HgImportRequestQueue{};
  std::vector<Hash> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] =
        makeImportRequest(ImportPriority(ImportPriorityKind::Normal, i));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [smallHash, smallRequest] =
      makeImportRequest(ImportPriority(ImportPriorityKind::Low, 0));
  queue.enqueue(std::move(smallRequest));

  // the queue should give requests in the reverse order of pushing
  while (!enqueued.empty()) {
    auto expected = enqueued.back();
    enqueued.pop_back();

    auto request = queue.dequeue();

    EXPECT_EQ(expected, request->getHash());
  }

  EXPECT_EQ(smallHash, queue.dequeue()->getHash());
}

TEST(HgImportRequestQueueTest, getRequestByPriorityReverse) {
  auto queue = HgImportRequestQueue{};
  std::deque<Hash> enqueued;

  for (int i = 0; i < 10; i++) {
    auto [hash, request] =
        makeImportRequest(ImportPriority(ImportPriorityKind::Normal, 10 - i));

    queue.enqueue(std::move(request));
    enqueued.push_back(hash);
  }

  auto [largeHash, largeRequest] =
      makeImportRequest(ImportPriority(ImportPriority::kHigh()));
  queue.enqueue(std::move(largeRequest));
  EXPECT_EQ(largeHash, queue.dequeue()->getHash());

  while (!enqueued.empty()) {
    auto expected = enqueued.front();
    enqueued.pop_front();

    auto request = queue.dequeue();

    EXPECT_EQ(expected, request->getHash());
  }
}
