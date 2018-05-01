// Copyright Facebook, Inc. 2017
//! Errors.

error_chain! {
    errors {
        NotAStoreFile {
            description("the provided store file is not a valid store file"),
        }
        UnsupportedTreeVersion(v: u32) {
            description("tree version not supported"),
            display("tree version not supported: {}", v),
        }
        UnsupportedVersion(v: u32) {
            description("store file version not supported"),
            display("store file version not supported: {}", v),
        }
        InvalidStoreId(id: u64) {
            description("invalid store id"),
            display("invalid store id: {}", id),
        }
        ReadOnlyStore {
            description("store is read-only"),
        }
        CorruptTree {
            description("treedirstate is corrupt"),
        }
        CallbackError(desc: String) {
            description("callback error"),
            display("callback error: {}", desc),
        }
    }
    foreign_links {
        Io(::std::io::Error);
        Utf8(::std::str::Utf8Error);
        Utf8String(::std::string::FromUtf8Error);
    }
}
