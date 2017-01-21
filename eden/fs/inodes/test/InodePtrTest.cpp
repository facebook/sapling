/*
 *  Copyright (c) 2016-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#include <gtest/gtest.h>
#include "eden/fs/inodes/FileInode.h"
#include "eden/fs/inodes/InodeBase.h"
#include "eden/fs/inodes/InodePtr.h"
#include "eden/fs/inodes/TreeInode.h"
#include "eden/fs/testharness/TestMount.h"
#include "eden/fs/testharness/TestUtil.h"
#include "eden/utils/test/TestChecks.h"

using namespace facebook::eden;

namespace facebook {
namespace eden {
/*
 * InodePtrTestHelper is declared as a friend by InodeBase.
 *
 * We can use this class to contain any functions that need to access private
 * inode state for the purpose of our tests.
 */
class InodePtrTestHelper {
 public:
  template <typename InodePtrType>
  static uint32_t getRefcount(const InodePtrType& inode) {
    return inode->ptrRefcount_.load(std::memory_order_acquire);
  }
};
}
}

#define EXPECT_REFCOUNT(expected, inodePtr) \
  EXPECT_EQ(expected, InodePtrTestHelper::getRefcount(inodePtr))

TEST(InodePtr, constructionAndAssignment) {
  TestMountBuilder builder;
  auto testMount = builder.build();

  // Get the root inode
  auto rootPtr = testMount->getEdenMount()->getRootInode();
  // The refcount for the root should be 2:
  // - The InodeMap keeps 1 reference to the root inode
  // - We got a second reference
  EXPECT_REFCOUNT(2, rootPtr);
  EXPECT_TRUE(rootPtr);

  {
    // Construction through newPtrFromExisting()
    auto ptr2 = TreeInodePtr::newPtrFromExisting(rootPtr.get());
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_EQ(rootPtr.get(), ptr2.get());
    // reset()
    ptr2.reset();
    EXPECT_REFCOUNT(2, rootPtr);
    EXPECT_FALSE(ptr2);
  }

  {
    // Copy construction
    auto ptr2 = rootPtr;
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_EQ(rootPtr.get(), ptr2.get());
  }
  // Decrement via destruction
  EXPECT_REFCOUNT(2, rootPtr);

  {
    // Default construction, then copy assignment
    TreeInodePtr ptr2;
    EXPECT_FALSE(ptr2);
    EXPECT_REFCOUNT(2, rootPtr);
    ptr2 = rootPtr;
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_TRUE(ptr2);
    EXPECT_EQ(rootPtr.get(), ptr2.get());

    // Move construction
    TreeInodePtr ptr3{std::move(ptr2)};
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_EQ(rootPtr.get(), ptr3.get());
    EXPECT_TRUE(ptr3);
    EXPECT_TRUE(nullptr == ptr2.get());
    EXPECT_FALSE(ptr2);

    // Move assignment
    ptr2 = std::move(ptr3);
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_EQ(rootPtr.get(), ptr2.get());
    EXPECT_TRUE(ptr2);
    EXPECT_TRUE(nullptr == ptr3.get());
    EXPECT_FALSE(ptr3);

    // Try move assigning to the value it already points to
    // This effectively decrements the refcount since the right-hand side gets
    // reset but the left-hand side of the assignment stays the same.
    ptr3 = rootPtr;
    EXPECT_REFCOUNT(4, rootPtr);
    ptr2 = std::move(ptr3);
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_FALSE(ptr3);
    EXPECT_TRUE(ptr2);
    EXPECT_EQ(rootPtr.get(), ptr2.get());
  }
  EXPECT_REFCOUNT(2, rootPtr);

  {
    // Copy assignment from null
    // First set ptr2 to non-null
    TreeInodePtr ptr2 = rootPtr;
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_TRUE(ptr2);
    TreeInodePtr nullTreePtr;
    ptr2 = nullTreePtr;
    EXPECT_REFCOUNT(2, rootPtr);
    EXPECT_FALSE(ptr2);
    EXPECT_FALSE(nullTreePtr);

    // Move assignment from null
    // First set ptr2 to non-null
    ptr2 = rootPtr;
    EXPECT_REFCOUNT(3, rootPtr);
    EXPECT_TRUE(ptr2);
    ptr2 = std::move(nullTreePtr);
    EXPECT_REFCOUNT(2, rootPtr);
    EXPECT_FALSE(ptr2);
    EXPECT_FALSE(nullTreePtr);

    // Copy construction from null
    TreeInodePtr ptr4{nullTreePtr};
    EXPECT_REFCOUNT(2, rootPtr);
    EXPECT_FALSE(ptr4);

    // Move construction from null
    TreeInodePtr ptr5{std::move(nullTreePtr)};
    EXPECT_REFCOUNT(2, rootPtr);
    EXPECT_FALSE(ptr5);
  }
  EXPECT_REFCOUNT(2, rootPtr);
}

TEST(InodePtr, baseConstructionAndAssignment) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto rootPtr = testMount->getEdenMount()->getRootInode();
  EXPECT_REFCOUNT(2, rootPtr);

  // Construct an InodePtr from TreeInodePtr
  InodePtr basePtr = rootPtr;
  EXPECT_REFCOUNT(3, rootPtr);
  EXPECT_EQ(rootPtr.get(), basePtr.get());
  EXPECT_TRUE(basePtr);

  {
    // Move construction from TreeInodePtr
    TreeInodePtr root2 = rootPtr;
    EXPECT_REFCOUNT(4, rootPtr);
    EXPECT_EQ(rootPtr.get(), root2.get());
    InodePtr basePtr2(std::move(root2));
    EXPECT_REFCOUNT(4, rootPtr);
    EXPECT_EQ(rootPtr.get(), basePtr2.get());
    EXPECT_TRUE(basePtr2);
    EXPECT_FALSE(root2);

    // Copy assignment from TreeInodePtr
    InodePtr basePtr3;
    EXPECT_FALSE(basePtr3);
    basePtr3 = rootPtr;
    EXPECT_TRUE(basePtr3);
    EXPECT_TRUE(rootPtr);
    EXPECT_EQ(rootPtr.get(), basePtr3.get());
    EXPECT_REFCOUNT(5, rootPtr);

    // Move assignment from TreeInodePtr
    basePtr3.reset();
    EXPECT_REFCOUNT(4, rootPtr);
    root2 = rootPtr;
    EXPECT_REFCOUNT(5, rootPtr);
    EXPECT_FALSE(basePtr3);
    basePtr3 = std::move(root2);
    EXPECT_TRUE(basePtr3);
    EXPECT_FALSE(root2);
    EXPECT_REFCOUNT(5, rootPtr);

    // Try move assigning to the value it already points to
    root2 = rootPtr;
    EXPECT_REFCOUNT(6, rootPtr);
    basePtr3 = std::move(root2);
    EXPECT_REFCOUNT(5, rootPtr);
    EXPECT_FALSE(root2);
    EXPECT_TRUE(basePtr3);
    EXPECT_EQ(rootPtr.get(), basePtr3.get());
  }
  EXPECT_REFCOUNT(3, rootPtr);
}

TEST(InodePtr, baseCasting) {
  TestMountBuilder builder;
  auto testMount = builder.build();
  auto rootPtr = testMount->getEdenMount()->getRootInode();
  EXPECT_REFCOUNT(2, rootPtr);

  // Construct an InodePtr from TreeInodePtr
  InodePtr basePtr = rootPtr;
  EXPECT_REFCOUNT(3, rootPtr);

  // Test the various asTree* methods
  {
    // Raw pointer versions
    EXPECT_EQ(rootPtr.get(), basePtr.get());
    EXPECT_EQ(rootPtr.get(), basePtr.asTree());
    EXPECT_EQ(rootPtr.get(), basePtr.asTreeOrNull());
    EXPECT_REFCOUNT(3, rootPtr);
  }
  {
    // Copy versions
    auto rawPtr = basePtr.asTree();
    auto rawPtr2 = basePtr.asTreeOrNull();
    EXPECT_REFCOUNT(3, rootPtr);
    auto ptr2 = basePtr.asTreePtr();
    EXPECT_TRUE(basePtr);
    EXPECT_EQ(rootPtr.get(), basePtr.get());
    EXPECT_EQ(rootPtr.get(), ptr2.get());
    EXPECT_TRUE(ptr2);
    EXPECT_REFCOUNT(4, rootPtr);
    auto ptr3 = basePtr.asTreePtrOrNull();
    EXPECT_REFCOUNT(5, rootPtr);
  }
  EXPECT_REFCOUNT(3, rootPtr);
  {
    // Move versions
    auto base2 = basePtr;
    EXPECT_REFCOUNT(4, rootPtr);
    auto ptr2 = std::move(base2).asTreePtr();
    EXPECT_REFCOUNT(4, rootPtr);
    EXPECT_FALSE(base2);
    EXPECT_EQ(rootPtr.get(), ptr2.get());

    ptr2.reset();
    EXPECT_REFCOUNT(3, rootPtr);
    base2 = basePtr;
    EXPECT_REFCOUNT(4, rootPtr);
    ptr2 = std::move(base2).asTreePtrOrNull();
    EXPECT_REFCOUNT(4, rootPtr);
    EXPECT_FALSE(base2);
    EXPECT_EQ(rootPtr.get(), ptr2.get());
  }
  EXPECT_REFCOUNT(3, rootPtr);

  // Test the various asFile* methods
  {
    // Raw pointer versions
    EXPECT_EQ(rootPtr.get(), basePtr.get());
    EXPECT_THROW_ERRNO(basePtr.asFile(), EISDIR);
    EXPECT_TRUE(nullptr == basePtr.asFileOrNull());
    EXPECT_REFCOUNT(3, rootPtr);
    auto rawPtr = basePtr.asFileOrNull();
    EXPECT_TRUE(nullptr == rawPtr);
    EXPECT_REFCOUNT(3, rootPtr);
  }
  {
    // Copy versions
    EXPECT_THROW_ERRNO(basePtr.asFile(), EISDIR);
    EXPECT_THROW_ERRNO(basePtr.asFilePtr(), EISDIR);
    EXPECT_REFCOUNT(3, rootPtr);
    auto filePtr = basePtr.asFilePtrOrNull();
    EXPECT_FALSE(filePtr);
    EXPECT_REFCOUNT(3, rootPtr);
  }
  {
    // Move versions
    auto base2 = basePtr;
    EXPECT_REFCOUNT(4, rootPtr);

    EXPECT_THROW_ERRNO(std::move(base2).asFile(), EISDIR);
    EXPECT_TRUE(base2);
    EXPECT_REFCOUNT(4, rootPtr);

    EXPECT_THROW_ERRNO(std::move(base2).asFilePtr(), EISDIR);
    EXPECT_TRUE(base2);
    EXPECT_REFCOUNT(4, rootPtr);

    auto filePtr = std::move(base2).asFilePtrOrNull();
    EXPECT_FALSE(filePtr);
    EXPECT_TRUE(base2);
    EXPECT_REFCOUNT(4, rootPtr);
  }
  EXPECT_REFCOUNT(3, rootPtr);
}
