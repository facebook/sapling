/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#pragma once

#include <sys/mman.h>
#include <unistd.h>
#include <type_traits>

#include <eden/fs/utils/Bug.h>
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

/**
 * Enforce required properties of
 */
template <typename... T>
struct RecordTypeRequirements;

template <>
struct RecordTypeRequirements<> {
  using type = void;
};

template <typename T, typename... Rest>
struct RecordTypeRequirements<T, Rest...> {
  static_assert(
      std::is_standard_layout<T>::value,
      "Records must have standard layout");
  static_assert(
      std::is_trivially_destructible<T>::value,
      "MappedDiskVector does not support custom destructors");
  static_assert(
      std::is_trivially_move_assignable<T>::value,
      "Records will be relocated in memory");
  static_assert(
      std::is_convertible<typeof(T::VERSION), uint32_t>::value,
      "Record's VERSION constant must convert to a uint32_t");
  static_assert(T::VERSION >= 0, "Record VERSION cannot be negative");
  static_assert(
      T::VERSION < std::numeric_limits<uint32_t>::max(),
      "Record VERSION must fit in 32 bits");

  using type = typename RecordTypeRequirements<Rest...>::type;
};

template <typename T, typename... OldVersions>
struct Migrator;
} // namespace detail

/**
 * MappedDiskVector is roughly analogous to std::vector, except it's backed by
 * a persistent memory-mapped file.
 *
 * MappedDiskVector is not thread-safe - the caller is
 * responsible for synchronization. It is safe for multiple threads to
 * simultaneously read, however.
 *
 * While alive, MappedDiskVector does acquire an exclusive flock on the
 * underlying fd to avoid multiple processes manipulating it at the same time.
 *
 * MappedDiskVector supports migrating from old formats to new formats via the
 * OldVersions template parameter. For any given type T, T::VERSION is written
 * into the header and used for version negotiation. sizeof(T) is also recorded
 * to prevent accidentally adding a field without changing the version.
 *
 * MappedDiskVector<A, B, C> will, if decoding the file as A fails, try to
 * decode as B and C, and if either succeeds, the file will be migrated to the
 * new format and reopened. In particular, each record will be constructed with
 * an instance of the type of the right. When migrating from C to A above,
 * the new file will contain values constructed with C{B{oldA}}.
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
    typename = typename detail::RecordTypeRequirements<T>::type>
class MappedDiskVector {
 public:
  /**
   * Opens or creates the MappedDiskVector at the specified path.  The path is
   * only used to open the file - a single file descriptor is used from then on
   * with the underlying inode resized in place.
   *
   * If the load fails because of a version mismatch, the types specified in
   * OldVersions are tried sequentially. If one succeeds, the entries are
   * converted one-by-one into the new format and the new table replaces the
   * old.
   */
  template <typename... OldVersions>
  static MappedDiskVector open(
      folly::StringPiece path,
      bool shouldPopulate = false) {
    folly::File file{path, O_RDWR | O_CREAT | O_CLOEXEC, 0600};

    if (!file.try_lock()) {
      folly::throwSystemError("failed to acquire lock on ", path);
    }

    struct stat st;
    folly::checkUnixError(
        fstat(file.fd(), &st), "fstat failed on MappedDiskVector path ", path);

    if (st.st_size == 0) {
      return initializeFromScratch(std::move(file));
    }

    Header header;
    ssize_t readBytes =
        folly::preadNoInt(file.fd(), &header, sizeof(header), 0);
    if (readBytes == -1) {
      folly::throwSystemError("failed to read MappedDiskVector header");
    } else if (readBytes != sizeof(header)) {
      XLOG(WARNING) << "file contains incomplete header: only read "
                    << readBytes << " bytes";
      throw std::runtime_error("Incomplete MappedDiskVector header");
    }

    if (kMagic != header.magic || header.version != 1 ||
        static_cast<ssize_t>(sizeof(header)) > st.st_size ||
        header.recordSize == 0 ||
        // careful not to overflow by multiplying entryCount by recordSize
        header.entryCount > (st.st_size - sizeof(header)) / header.recordSize ||
        header.unused != 0) {
      throw std::runtime_error(
          "Invalid header: this is probably not a MappedDiskVector file");
    }

    // Verify that every given record type has a unique VERSION value.
    // This check could be done at compile time.
    static constexpr std::array<uint32_t, 1 + sizeof...(OldVersions)> versions =
        {T::VERSION, OldVersions::VERSION...};
    for (size_t i = 0; i < versions.size(); ++i) {
      for (size_t j = i + 1; j < versions.size(); ++j) {
        if (versions[i] == versions[j]) {
          throw std::logic_error(folly::to<std::string>(
              "Duplicate VERSION detected in record types: ", versions[i]));
        }
      }
    }

    // Does this file match the primary record type? If so, we're done.
    if (T::VERSION == header.recordVersion) {
      if (sizeof(T) != header.recordSize) {
        throw std::runtime_error(folly::to<std::string>(
            "Record size does not match size recorded in file. Expected ",
            sizeof(T),
            " but file has ",
            header.recordSize));
      }
      return MappedDiskVector{
          std::move(file), st.st_size, header.entryCount, shouldPopulate};
    }

    // Try to migrate from an old record format if any match.
    static constexpr std::array<size_t, sizeof...(OldVersions)> sizes = {
        sizeof(OldVersions)...};
    for (size_t i = 0; i < sizes.size(); ++i) {
      if (versions[i + 1] == header.recordVersion) {
        if (sizes[i] != header.recordSize) {
          throw std::runtime_error(folly::to<std::string>(
              "Record version matches old record type but record size differs. ",
              "Expected ",
              sizes[i],
              " but file has ",
              header.recordSize));
        }
        return detail::Migrator<T, OldVersions...>::migrateFrom(
            path,
            std::move(file),
            st.st_size,
            header.entryCount,
            i,
            [](const auto& from) { return T{from}; });
      }
    }

    throw std::runtime_error(folly::to<std::string>(
        "Unexpected record size and version. "
        "Expected size=",
        sizeof(T),
        ", version=",
        T::VERSION,
        " but got size=",
        header.recordSize,
        ", version=",
        header.recordVersion));
  }

  /**
   * Creates a new MappedDiskVector at the specified path, overwriting any that
   * was there prior.
   */
  static MappedDiskVector createOrOverwrite(folly::StringPiece path) {
    folly::File file{
        path, O_RDWR | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0600};
    if (!file.try_lock()) {
      folly::throwSystemError("failed to acquire lock on ", path);
    }

    return initializeFromScratch(std::move(file));
  }

  MappedDiskVector() = delete;
  MappedDiskVector(const MappedDiskVector&) = delete;
  MappedDiskVector& operator=(const MappedDiskVector&) = delete;

  MappedDiskVector(MappedDiskVector&& other) : file_(std::move(other.file_)) {
    begin_ = other.begin_;
    end_ = other.end_;
    map_ = other.map_;
    mapSizeInBytes_ = other.mapSizeInBytes_;

    other.begin_ = nullptr;
    other.end_ = nullptr;
    other.map_ = nullptr;
    other.mapSizeInBytes_ = 0;
  }

  MappedDiskVector& operator=(MappedDiskVector&& other) {
    if (map_) {
      munmap(map_, mapSizeInBytes_);
    }

    file_ = std::move(other.file_);
    begin_ = other.begin_;
    end_ = other.end_;
    map_ = other.map_;
    mapSizeInBytes_ = other.mapSizeInBytes_;

    other.begin_ = nullptr;
    other.end_ = nullptr;
    other.map_ = nullptr;
    other.mapSizeInBytes_ = 0;
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

#ifdef __APPLE__
      auto newMap = mmap(
          nullptr,
          newFileSize,
          PROT_READ | PROT_WRITE,
          MAP_SHARED,
          file_.fd(),
          0);
#else
      auto newMap = mremap(map_, mapSizeInBytes_, newFileSize, MREMAP_MAYMOVE);
#endif
      if (newMap == MAP_FAILED) {
        folly::throwSystemError(folly::to<std::string>(
            "mremap failed when growing capacity from ",
            mapSizeInBytes_,
            " to ",
            newFileSize));
      }

#ifdef __APPLE__
      munmap(map_, mapSizeInBytes_);
#endif
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
    uint32_t recordVersion; // T::VERSION
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

  static MappedDiskVector initializeFromScratch(folly::File file) {
    // Start the file large enough to handle the header and a little under one
    // round one of growth.
    constexpr size_t initialSize = GROWTH_IN_PAGES * detail::kPageSize;
    static_assert(
        initialSize >= sizeof(Header) + sizeof(T),
        "Initial size must include enough space for the header and at least one element.");
    if (-1 == folly::ftruncateNoInt(file.fd(), initialSize)) {
      folly::throwSystemError(
          "failed to initialize MappedDiskVector: ftruncate() failed");
    }

    Header header;
    header.magic = kMagic;
    header.version = 1;
    header.recordVersion = T::VERSION;
    header.recordSize = sizeof(T);
    header.entryCount = 0;
    header.unused = 0;
    ssize_t written = folly::pwriteNoInt(file.fd(), &header, sizeof(header), 0);
    if (-1 == written) {
      folly::throwSystemError("Failed to write initial header");
    }
    if (written != sizeof(header)) {
      throw std::runtime_error("Failed to write complete initial header");
    }

    return MappedDiskVector{
        std::move(file), initialSize, header.entryCount, false};
  }

  explicit MappedDiskVector(
      folly::File file,
      off_t fileSize,
      size_t currentEntryCount,
      bool populate)
      : file_(std::move(file)) {
    // It's worth keeping the file and mapping a whole number of pages to
    // avoid wasting an partial page at the end.  Note that this is an
    // optimization and it doesn't matter if kPageSize differs from the
    // system page size.
    size_t desiredSize = detail::roundUpToNonzeroPageSize(fileSize);
    if (fileSize != static_cast<ssize_t>(desiredSize)) {
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

    // Call readahead() here?  Offer it as optional functionality?
    // InodeTable needs to traverse every record immediately after opening.

    auto map = mmap(
        0,
        desiredSize,
        PROT_READ | PROT_WRITE,
        MAP_SHARED
#ifdef MAP_POPULATE
            | (populate ? MAP_POPULATE : 0)
#endif
            ,
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

  template <typename T_, typename... OldVersions>
  friend struct detail::Migrator;
};

namespace detail {
template <typename T>
struct Migrator<T> {
  template <typename ConvertFn>
  static MappedDiskVector<T> migrateFrom(
      folly::StringPiece /*path*/,
      folly::File /*file*/,
      off_t /*fileSize*/,
      size_t /*currentEntryCount*/,
      size_t /*oldVersionIndex*/,
      ConvertFn /*convert*/) {
    auto bug = EDEN_BUG() << "oldVersionIndex >= sizeof...(OldVersions)";
    bug.throwException();
  }
};

template <typename T, typename First, typename... Rest>
struct Migrator<T, First, Rest...> {
  template <typename ConvertFn>
  static MappedDiskVector<T> migrateFrom(
      folly::StringPiece path,
      folly::File file,
      off_t fileSize,
      size_t currentEntryCount,
      size_t oldVersionIndex,
      ConvertFn convert) {
    using namespace folly::literals;

    // static assert type of fn() is First -> T

    if (oldVersionIndex == 0) {
      // At this point, it's clear the original file is compatible with First.
      // Load it, migrate each element to a new temporary file, and move the
      // temporary file over the original.
      // Set populate to true because migrating requires reading every element
      // anyway.
      MappedDiskVector<First> original{
          std::move(file), fileSize, currentEntryCount, true};

      auto tmpPath = folly::to<std::string>(path, ".tmp");
      auto newVector = MappedDiskVector<T>::createOrOverwrite(tmpPath);
      try {
        // TODO: newVector.reserve
        for (size_t i = 0; i < original.size(); ++i) {
          newVector.emplace_back(convert(original[i]));
        }

        if (rename(tmpPath.c_str(), path.str().c_str())) {
          folly::throwSystemError(
              "rename() failed while migrating MDV formats");
        }

        return newVector;
      } catch (const std::exception&) {
        unlink(tmpPath.c_str());
        throw;
      }
    }

    return Migrator<T, Rest...>::migrateFrom(
        path,
        std::move(file),
        fileSize,
        currentEntryCount,
        oldVersionIndex - 1,
        [=](const auto& from) { return convert(First{from}); });
  }
};

} // namespace detail

} // namespace eden
} // namespace facebook
