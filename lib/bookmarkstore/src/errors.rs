use atomicwrites;

error_chain! {
    errors {
        DuplicateBookmark(b: String) {
            description("duplicate bookmark name"),
            display("duplicate bookmark name: {}", b),
        }

        BookmarkNotFound(b: String) {
            description("bookmark not found"),
            display("bookmark not found: {}", b),
        }

        MalformedBookmarkFile(line_num: u32,  line: String) {
            description("malformed bookmark file"),
            display("malformed bookmark file at line {}: {}", line_num, line),
        }
    }
    foreign_links {
        Io(::std::io::Error);
        AtomicWrites(atomicwrites::Error::<::std::io::Error>);
    }
}
