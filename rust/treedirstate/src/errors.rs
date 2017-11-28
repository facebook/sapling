// Copyright Facebook, Inc. 2017
//! Errors.

error_chain! {
    errors {
        InvalidStoreId(id: u64) {
            description("invalid store id"),
            display("invalid store id: {}", id),
        }
    }
    foreign_links {
        Io(::std::io::Error);
        Utf8(::std::str::Utf8Error);
        Utf8String(::std::string::FromUtf8Error);
    }
}
