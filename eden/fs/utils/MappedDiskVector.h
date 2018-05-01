/*
 *  Copyright (c) 2018-present, Facebook, Inc.
 *  All rights reserved.
 *
 *  This source code is licensed under the BSD-style license found in the
 *  LICENSE file in the root directory of this source tree. An additional grant
 *  of patent rights can be found in the PATENTS file in the same directory.
 *
 */
#pragma once

#include <sys/mman.h>
#include <unistd.h>
#include <type_traits>

#include <folly/Exception.h>
#include <folly/File.h>
#include <folly/FileUtil.h>
#include <folly/Range.h>
#include <folly/logging/xlog.h>

namespace facebook {
namespace eden {

namespace detail {

/**
 * The precise value of kPageSize doesn't matter for correctness.  It's used
 * primarily as a microoptimization - MappedDiskVector attempts to avoid mapping
 * fractions of pages which lets it resize the file a bit less often.
 */
constexpr size_t kPageSize = 4096;

inline size_t roundUpToNonzeroPageSize(size_t s) {
  static_assert(
      0 == (kPageSize & (kPageSize - 1)), "kPageSize must be power of two");
  return std::max(kPageSize, (s + kPageSize - 1) & ~(kPageSize - 1));
}
} // namespace detail

/**
 * MappedDiskVector is roughly analogous to std::vector, except it's backed by
 * a persistent memory-mapped file.
 *
 * MappedDiskVector is not thread-safe - the caller is
 * responsible for synchronization.  It is safe for multiple threads to read
 * simultaneously, though.
 *
 * While alive, MappedDiskVector does acquire an exclusive lock on the
 * underlying fd to avoid multiple processes manipulating it at the same time.
 *
 * Future work:
 *
 * This type needs to be split into two: the non-template, untyped storage
 * class that manages resizing the file and mapping and parsing the header,
 * and the typed view that owns the storage and exposes it as a typed vector.
 *
 * The caller needs the ability to read userVersion and negotiate an upgrade.
 * I imagine a new type that owns the file handle and allows reading the file
 * size and header, independent of the desired T.  The call can use entrySize
 * and/or userVersion to decide whether to migrate the file's contents into
 * a new file.  Then this type would be moved into a new MappedDiskVector<T>
 * and accessed as a vector afterwards.
 */
template <
    typename T,
    typename = std::enable_if_t<
        std::is_standard_layout<T>::value &&
        std::is_trivially_destructible<T>::value &&
        std::is_trivially_move_assignable<T>::value>>
class MappedDiskVector {
 public:
  MappedDiskVector() = delete;
  MappedDiskVector(const MappedDiskVector&) = delete;
  MappedDiskVector& operator=(const MappedDiskVector&) = delete;

  // There's no inherent reason this type could not be movable.  It just hasn't
  // been necessary yet.
  MappedDiskVector(MappedDiskVector&&) = delete;
  MappedDiskVector& operator=(MappedDiskVector&&) = delete;

  /**
   * Opens or recreates the MappedDiskVector at the specified path.  The path is
   * only used to open the file - a single file descriptor is used from then on
   * with the underlying inode resized in place.
   */
  explicit MappedDiskVector(
      folly::StringPiece path,
      bool shouldPopulate = false)
      : file_(path, O_RDWR | O_CREAT | O_CLOEXEC, 0600) {
    if (!file_.try_lock()) {
      folly::throwSystemError("failed to acquire lock on ", path);
    }

    struct stat st;
    folly::checkUnixError(
        fstat(file_.fd(), &st), "fstat failed on MappedDiskVector path ", path);

    if (st.st_size == 0) {
      initializeFromScratch();
      return;
    }

    Header header;
    ssize_t readBytes =
        folly::preadNoInt(file_.fd(), &header, sizeof(header), 0);
    if (readBytes == -1) {
      folly::throwSystemError("failed to read MappedDiskVector header");
    } else if (readBytes != sizeof(header)) {
      XLOG(WARNING) << "file contains incomplete header: only read "
                    << readBytes << " bytes";
      throw std::runtime_error("Include MappedDiskVector header");
    }

    if (kMagic != header.magic || header.version != 1 ||
        sizeof(header) > st.st_size ||
        // careful not to overflow by multiplying entryCount by sizeof(T)
        header.entryCount > (st.st_size - sizeof(header)) / sizeof(T) ||
        header.recordSize != sizeof(T) || header.unused != 0) {
      throw std::runtime_error(
          "Invalid header: this is probably not a MappedDiskVector file");
    }

    createMap(st.st_size, header.entryCount, shouldPopulate);
  }

  ~MappedDiskVector() {
    if (map_) {
      munmap(map_, mapSizeInBytes_);
    }
  }

  size_t size() const {
    return end_ - begin_;
  }

  size_t capacity() const {
    // round down
    return (mapSizeInBytes_ - sizeof(Header)) / sizeof(T);
  }

  T& operator[](size_t index) {
    return begin_[index];
  }

  const T& operator[](size_t index) const {
    return begin_[index];
  }

  template <typename... Args>
  void emplace_back(Args&&... args) {
    if (!hasRoom(1)) {
      static_assert(
          sizeof(GROWTH_IN_PAGES) * detail::kPageSize >= sizeof(T),
          "Growth must expand the file more than a single record");

      size_t oldSize = size();
      size_t newFileSize =
          mapSizeInBytes_ + GROWTH_IN_PAGES * detail::kPageSize;

      // Always keep the file size a whole number of pages.
      CHECK_EQ(0, newFileSize % detail::kPageSize);

      if (-1 == folly::ftruncateNoInt(file_.fd(), newFileSize)) {
        folly::throwSystemError("ftruncateNoInt failed when growing capacity");
      }

      auto newMap = mremap(map_, mapSizeInBytes_, newFileSize, MREMAP_MAYMOVE);
      if (newMap == MAP_FAILED) {
        folly::throwSystemError(folly::to<std::string>(
            "mremap failed when growing capacity from ",
            mapSizeInBytes_,
            " to ",
            newFileSize));
      }

      map_ = newMap;
      mapSizeInBytes_ = newFileSize;

      begin_ = reinterpret_cast<T*>(static_cast<Header*>(newMap) + 1);
      end_ = begin_ + oldSize;
    }

    T* out = end_;
    new (out) T{std::forward<Args>(args)...}; // may throw
    end_ = out + 1;

    ++header().entryCount;
  }

  void pop_back() {
    // TODO: It might be worth eliminating the end_ pointer and always adding
    // header().entryCount to begin_.
    DCHECK_GT(end_, begin_);
    --end_;
    --header().entryCount;
  }

  T& front() {
    DCHECK_GT(end_, begin_);
    return begin_[0];
  }

  T& back() {
    DCHECK_GT(end_, begin_);
    return end_[-1];
  }

 private:
  static constexpr uint32_t kMagic = 0x0056444d; // "MDV\0"

  struct Header {
    uint32_t magic;
    uint32_t version; // 1
    uint32_t userVersion; // bubbled up to user
    uint32_t recordSize; // sizeof(T)
    uint64_t entryCount; // end() - begin()
    uint64_t unused; // for alignment
  };
  static_assert(
      32 == sizeof(Header),
      "changing the header size would invalidate all files");
  static_assert(
      0 == sizeof(Header) % 16,
      "header alignment is 16 bytes in case someone uses SSE values");

  static constexpr size_t GROWTH_IN_PAGES = 256;

  void initializeFromScratch() {
    // Start the file large enough to handle the header and a little under one
    // round one of growth.
    constexpr size_t initialSize = GROWTH_IN_PAGES * detail::kPageSize;
    static_assert(
        initialSize >= sizeof(Header) + sizeof(T),
        "Initial size must include enough space for the header and at least one element.");
    if (-1 == folly::ftruncateNoInt(file_.fd(), initialSize)) {
      folly::throwSystemError(
          "failed to initialize MappedDiskVector: ftruncate() failed");
    }

    Header header;
    header.magic = kMagic;
    header.version = 1;
    header.userVersion = 0;
    header.recordSize = sizeof(T);
    header.entryCount = 0;
    header.unused = 0;
    ssize_t written =
        folly::pwriteNoInt(file_.fd(), &header, sizeof(header), 0);
    if (-1 == written) {
      folly::throwSystemError("Failed to write initial header");
    }
    if (written != sizeof(header)) {
      throw std::runtime_error("Failed to write complete initial header");
    }

    return createMap(initialSize, header.entryCount, false);
  }

  void createMap(off_t fileSize, size_t currentEntryCount, bool populate) {
    // It's worth keeping the file and mapping a whole number of pages to avoid
    // wasting an partial page at the end.  Note that this is an optimization
    // and it doesn't matter if kPageSize differs from the system page size.
    size_t desiredSize = detail::roundUpToNonzeroPageSize(fileSize);
    if (fileSize != desiredSize) {
      if (fileSize) {
        XLOG(WARNING)
            << "Warning: MappedDiskVector file size not multiple of page size: "
            << fileSize;
      }
      if (folly::ftruncateNoInt(file_.fd(), desiredSize)) {
        folly::throwSystemError(
            "ftruncateNoInt failed when rounding up to page size");
      }
    }

    // Call readahead() here?  Offer it as optional functionality?  InodeTable
    // needs to traverse every record immediately after opening.

    auto map = mmap(
        0,
        desiredSize,
        PROT_READ | PROT_WRITE,
        MAP_SHARED | (populate ? MAP_POPULATE : 0),
        file_.fd(),
        0);
    if (map == MAP_FAILED) {
      folly::throwSystemError("mmap failed on file open");
    }

    // Throw no exceptions between assigning the fields.

    map_ = map;
    mapSizeInBytes_ = desiredSize;
    static_assert(
        alignof(Header) >= alignof(T),
        "T must not have stricter alignment requirements than Header");
    begin_ = reinterpret_cast<T*>(static_cast<Header*>(map) + 1);
    end_ = begin_ + currentEntryCount;

    // Just double-check that the accessed region is within the map.
    CHECK_LE(
        reinterpret_cast<char*>(end_),
        static_cast<char*>(map_) + mapSizeInBytes_);
  }

  bool hasRoom(size_t amount) const {
    // Technically, the expression (end_ + amount) is constructing a pointer
    // past the end of the "object" (mmap) and is thus UB.  But hopefully no
    // compiler can see that.
    return reinterpret_cast<char*>(end_ + amount) <=
        static_cast<char*>(map_) + mapSizeInBytes_;
  }

  T* data() {
    return begin();
  }

  T* begin() {
    return begin_;
  }

  const T* begin() const {
    return begin_;
  }

  T* end() {
    return end_;
  }

  const T* end() const {
    return end_;
  }

  Header& header() {
    return *static_cast<Header*>(map_);
  }

  const Header& header() const {
    return *static_cast<Header*>(map_);
  }

  // these two should be at the front of the struct
  T* begin_{nullptr};
  T* end_{nullptr};

  void* map_{nullptr};
  size_t mapSizeInBytes_{0}; // must be nonzero, multiple of page size

  folly::File file_;
};

} // namespace eden
} // namespace facebook
