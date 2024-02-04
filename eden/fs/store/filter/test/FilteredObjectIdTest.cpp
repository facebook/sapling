/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#include <folly/Range.h>
#include <folly/logging/xlog.h>
#include <folly/portability/GTest.h>
#include <stdexcept>

#include "eden/fs/model/ObjectId.h"
#include "eden/fs/store/filter/FilteredObjectId.h"
#include "eden/fs/utils/PathFuncs.h"

using namespace facebook::eden;

TEST(FilteredObjectIdTest, test_blob) {
  std::string objectIdString = "deadbeeffacebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};

  FilteredObjectId filterId{object, FilteredObjectIdType::OBJECT_TYPE_BLOB};

  EXPECT_EQ(filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_BLOB);
  EXPECT_EQ(filterId.object(), object);
}

TEST(FilteredObjectIdTest, test_blob_getters_throw) {
  std::string objectIdString = "deadbeef facebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};

  FilteredObjectId filterId{object, FilteredObjectIdType::OBJECT_TYPE_BLOB};

  // Blob objects don't have paths/filters associated with them. Using the
  // getters results in an exception.
  EXPECT_EQ(filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_BLOB);
  EXPECT_THROW(filterId.filter(), std::invalid_argument);
  EXPECT_THROW(filterId.path(), std::invalid_argument);
}

TEST(FilteredObjectIdTest, test_unfiltered_tree) {
  std::string objectIdString = "deadbeeffacebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};

  FilteredObjectId filterId{
      object, FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE};

  EXPECT_EQ(
      filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE);
  EXPECT_EQ(filterId.object(), object);
}

TEST(FilteredObjectIdTest, test_unfiltered_tree_getters_throw) {
  std::string objectIdString = "deadbeef facebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};

  FilteredObjectId filterId{
      object, FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE};

  // Unfiltered tree objects don't have paths/filters associated with them.
  // Using the getters results in an exception.
  EXPECT_EQ(
      filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_UNFILTERED_TREE);
  EXPECT_THROW(filterId.filter(), std::invalid_argument);
  EXPECT_THROW(filterId.path(), std::invalid_argument);
}

TEST(FilteredObjectIdTest, test_tree_short_filter_and_path) {
  std::string objectIdString = "deadbeef facebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};
  std::string filterSet = "filterset";
  auto pathPiece =
      RelativePath{"this is a long enough string to push past SSO"};

  FilteredObjectId filterId{pathPiece, filterSet, object};

  EXPECT_EQ(filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_TREE);
  EXPECT_EQ(filterId.path(), pathPiece);
  EXPECT_EQ(filterId.filter(), filterSet);
  EXPECT_EQ(filterId.object(), object);
}

TEST(FilteredObjectIdTest, test_tree_long_filter_and_path) {
  std::string objectIdString = "deadbeef facebooc";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};
  std::string filterSet =
      "This filterset is very long. Some would say it's longer than 255 characters. "
      "This filterset is very long. Some would say it's longer than 255 characters. "
      "This filterset is very long. Some would say it's longer than 255 characters. "
      "This filterset is very long. Some would say it's longer than 255 characters. "
      "This filterset is very long. Some would say it's longer than 255 characters. ";
  auto pathPiece = RelativePath{
      "This is a very long string that is greater than 255 chars"
      "This is a very long string that is greater than 255 chars"
      "This is a very long string that is greater than 255 chars"
      "This is a very long string that is greater than 255 chars"
      "This is a very long string that is greater than 255 chars"};

  FilteredObjectId filterId{pathPiece, filterSet, object};

  EXPECT_EQ(filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_TREE);
  EXPECT_EQ(filterId.path(), pathPiece);
  EXPECT_EQ(filterId.filter(), filterSet);
  EXPECT_EQ(filterId.object(), object);
}

TEST(FilteredObjectIdTest, test_copy_and_move) {
  std::string objectIdString = "objectid";
  folly::ByteRange objectIdBytes{folly::StringPiece{objectIdString}};
  ObjectId object{objectIdBytes};
  std::string filterSet = "filterset";
  auto pathPiece = RelativePath{"a path piece"};

  FilteredObjectId filterId{pathPiece, filterSet, object};
  FilteredObjectId filterIdCopy{filterId};
  EXPECT_EQ(filterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_TREE);
  EXPECT_EQ(filterIdCopy.objectType(), FilteredObjectIdType::OBJECT_TYPE_TREE);
  EXPECT_EQ(filterId, filterIdCopy);

  FilteredObjectId movedFilterId{std::move(filterId)};
  EXPECT_EQ(movedFilterId.objectType(), FilteredObjectIdType::OBJECT_TYPE_TREE);
  EXPECT_EQ(movedFilterId, movedFilterId);
}
