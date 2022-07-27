/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::Ordering;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::FsNodeMetadata;
use minibytes::Bytes;
use storemodel::TreeFormat;
pub use storemodel::TreeStore;
use types::HgId;
use types::Key;
use types::PathComponent;
use types::PathComponentBuf;
use types::RepoPath;

#[derive(Clone)]
pub struct InnerStore {
    tree_store: Arc<dyn TreeStore + Send + Sync>,
}

impl InnerStore {
    pub fn new(tree_store: Arc<dyn TreeStore + Send + Sync>) -> Self {
        InnerStore { tree_store }
    }

    pub fn format(&self) -> TreeFormat {
        self.tree_store.format()
    }

    pub fn get_entry(&self, path: &RepoPath, hgid: HgId) -> Result<Entry> {
        tracing::debug_span!(
            "tree::store::get",
            id = AsRef::<str>::as_ref(&hgid.to_hex())
        )
        .in_scope(|| {
            let bytes = self.tree_store.get(path, hgid)?;
            Ok(Entry(bytes, self.tree_store.format()))
        })
    }

    pub fn insert_entry(&self, path: &RepoPath, hgid: HgId, entry: Entry) -> Result<()> {
        tracing::debug_span!(
            "tree::store::insert",
            path = path.as_str(),
            id = AsRef::<str>::as_ref(&hgid.to_hex())
        )
        .in_scope(|| self.tree_store.insert(path, hgid, entry.0))
    }

    pub fn prefetch(&self, keys: impl IntoIterator<Item = Key>) -> Result<()> {
        let keys: Vec<Key> = keys.into_iter().collect();
        tracing::debug_span!(
            "tree::store::prefetch",
            ids = {
                let ids: Vec<String> = keys.iter().map(|k| k.hgid.to_hex()).collect();
                &AsRef::<str>::as_ref(&ids.join(" "))
            }
        )
        .in_scope(|| self.tree_store.prefetch(keys))
    }
}

/// The `Entry` is the data that is stored on disk. It should be seen as opaque to whether it
/// represents serialized or deserialized data. It is the object that performs the bytes to
/// business object transformations. It provides method for interacting with parsed data.
///
/// The ABNF specification for the current serialization is:
/// Entry         = 1*( Element LF )
/// Element       = PathComponent %x00 HgId [ Flag ]
/// Flag          = %s"x" / %s"l" / %s"t"
/// PathComponent = 1*( %x01-%x09 / %x0B-%xFF )
/// HgId          = 40HEXDIG
///
/// In this case an `Entry` is equivalent to the contents of a directory. The elements of the
/// directory are described by `Element`. `Entry` is a list of serialized `Element`s that are
/// separated by `\n`. An `Element` will always have a name (`PathComponent`) and a hash (`HgId`).
/// `Elements` may be different types of files or they can be directories. The type of element is
/// described by the flag or the absence of the flag. When the flag is missing we have a regular
/// file, the various flag options are: `x` for executable, `l` for symlink and `d` for directory.
/// It should be noted that Nodes are represented in their hex format rather than a straight up
/// binary format so they are 40 characters long rather than 20 bytes.
/// Check the documentation of the `PathComponent` struct for more details about it's
/// representation. For this serialization format it is important that they don't contain
/// `\0` or `\n`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Entry(pub minibytes::Bytes, pub TreeFormat);

pub struct EntryMut(Vec<u8>, TreeFormat);

/// The `Element` is a parsed element of a directory. Directory elements are either files either
/// direcotries. The type of element is signaled by `Flag`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Element {
    pub component: PathComponentBuf,
    pub hgid: HgId,
    pub flag: Flag,
}

/// Used to signal the type of element in a directory: file or directory.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Flag {
    File(FileType),
    Directory,
}

impl Entry {
    /// Returns an iterator over the elements that the current `Entry` contains. This is the
    /// primary method of inspection for an `Entry`.
    pub fn elements<'a>(&'a self) -> Elements<'a> {
        Elements::from_byte_slice(&self.0, self.1)
    }

    /// The primary builder of an Entry, from a list of `Element`.
    pub fn from_elements(mut elements: Vec<Element>, format: TreeFormat) -> Entry {
        let cmp = crate::namecmp::get_namecmp_func(format);
        elements.sort_unstable_by(|a, b| cmp(&a.component, a.flag, &b.component, b.flag));
        let mut underlying = Vec::new();
        match format {
            TreeFormat::Hg => {
                for element in elements.into_iter() {
                    underlying.extend(element.to_byte_vec_hg());
                    underlying.extend(b"\n");
                }
            }
            TreeFormat::Git => {
                for element in elements.into_iter() {
                    underlying.extend(element.to_byte_vec_git());
                }
            }
        }
        Entry(underlying.into(), format)
    }

    // used in tests, finalize and subtree_diff
    pub fn to_bytes(self) -> Bytes {
        self.0
    }
}

impl EntryMut {
    /// Constructs an empty `Entry`. It is not valid to save an empty `Entry`.
    pub fn new(format: TreeFormat) -> Self {
        EntryMut(Vec::new(), format)
    }

    /// Adds an element to the list of elements represented by this `Entry`.
    /// It is expected that elements are added sorted by paths.
    pub fn add_element_hg(&mut self, element: Element) {
        self.0.extend(element.to_byte_vec_hg());
        self.0.extend(b"\n");
    }

    pub fn freeze(self) -> Entry {
        Entry(self.0.into(), self.1)
    }
}

impl AsRef<[u8]> for Entry {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub struct Elements<'a> {
    byte_slice: &'a [u8],
    position: usize,
    format: TreeFormat,
}

impl<'a> Elements<'a> {
    /// Constructs `Elements` from raw byte slice.
    pub(crate) fn from_byte_slice(byte_slice: &'a [u8], format: TreeFormat) -> Self {
        Elements {
            byte_slice,
            position: 0,
            format,
        }
    }

    fn next_hg(&mut self) -> Option<Result<Element>> {
        if self.position >= self.byte_slice.len() {
            return None;
        }
        let end = match self.byte_slice[self.position..]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            None => {
                return Some(Err(format_err!(
                    "failed to deserialize tree manifest entry, missing line feed\n{:?}",
                    String::from_utf8_lossy(self.byte_slice)
                )));
            }
            Some(delta) => self.position + delta,
        };
        let result = Element::from_byte_slice_hg(&self.byte_slice[self.position..end]);
        self.position = end + 1;
        Some(result)
    }

    fn next_git(&mut self) -> Option<Result<Element>> {
        let slice = match self.byte_slice.get(self.position..) {
            None => return None,
            Some(s) if s.is_empty() => return None,
            Some(s) => s,
        };

        let (mode_len, name_len) = find_git_entry_positions(slice)?;

        let flag = match parse_git_mode(&slice[..mode_len]) {
            Ok(flag) => flag,
            Err(e) => return Some(Err(e)),
        };

        let mut offset = mode_len + 1;
        let name = &slice[offset..offset + name_len];
        let component = match PathComponent::from_utf8(name) {
            Ok(p) => p.to_owned(),
            Err(e) => return Some(Err(e.into())),
        };

        offset += name_len + 1;
        let hgid = if let Some(id_slice) = slice.get(offset..offset + HgId::len()) {
            HgId::from_slice(id_slice).expect("id_slice has the right length")
        } else {
            return Some(Err(format_err!("SHA1 is incomplete")));
        };

        offset += HgId::len();
        self.position += offset;

        let element = Element {
            component,
            hgid,
            flag,
        };
        Some(Ok(element))
    }

    /// Look up an item.
    /// This can be faster than checking `next()` entries if only called once.
    pub fn lookup(&self, name: &PathComponent) -> Result<Option<(HgId, Flag)>> {
        match self.format {
            TreeFormat::Hg => self.lookup_hg(name),
            TreeFormat::Git => self.lookup_git(name),
        }
    }

    fn lookup_hg(&self, name: &PathComponent) -> Result<Option<(HgId, Flag)>> {
        // NAME '\0' HEX_SHA1 MODE '\n'
        let mut slice: &[u8] = self.byte_slice;
        let name = {
            let name = name.as_byte_slice();
            let mut buf = Vec::with_capacity(name.len() + 1);
            buf.extend_from_slice(name);
            buf.push(b'\0');
            buf
        };
        while slice.len() >= name.len() {
            match slice[..name.len()].cmp(name.as_slice()) {
                Ordering::Less => {
                    // Check the next entry.
                    match slice.iter().skip(HgId::hex_len()).position(|&x| x == b'\n') {
                        Some(position) => {
                            slice = &slice[position + HgId::hex_len() + 1..];
                            continue;
                        }
                        None => break,
                    };
                }
                Ordering::Equal => {
                    let hex_start = name.len();
                    let hex_end = hex_start + HgId::hex_len();
                    let hex_slice = match slice.get(hex_start..hex_end) {
                        None => break,
                        Some(slice) => slice,
                    };
                    let hgid = HgId::from_hex(hex_slice)?;
                    let flag = parse_hg_flag(slice.get(hex_end))?;
                    return Ok(Some((hgid, flag)));
                }
                Ordering::Greater => break,
            }
        }
        Ok(None)
    }

    fn lookup_git(&self, name: &PathComponent) -> Result<Option<(HgId, Flag)>> {
        let mut slice: &[u8] = self.byte_slice;
        let name: &[u8] = name.as_byte_slice();
        while !slice.is_empty() {
            let (mode_len, name_len) = match find_git_entry_positions(slice) {
                Some(positions) => positions,
                None => return Ok(None),
            };
            let name_start = mode_len + 1;
            let name_end = name_start + name_len;
            let candidate = match slice.get(name_start..name_end) {
                Some(name) => name,
                None => break,
            };
            match candidate.cmp(name) {
                Ordering::Less => {}
                Ordering::Equal => {
                    let flag = parse_git_mode(&slice[..mode_len])?;
                    let id_start = name_end + 1;
                    let id_end = id_start + HgId::len();
                    let hgid = if let Some(id_slice) = slice.get(id_start..id_end) {
                        HgId::from_slice(id_slice).expect("id_slice has the right length")
                    } else {
                        return Err(format_err!("SHA1 is incomplete"));
                    };
                    return Ok(Some((hgid, flag)));
                }
                Ordering::Greater => {
                    // Directory names are tricky. See the `namecmp` module.
                    // Here we don't borther figuring out whehter it's a
                    // directory or not, just keep looking for a few more
                    // entires.
                    let len = candidate.len().min(name.len());
                    if candidate[..len] > name[..len] {
                        break;
                    }
                }
            }
            slice = match slice.get(name_end + 1 + HgId::len()..) {
                Some(slice) => slice,
                None => break,
            };
        }
        Ok(None)
    }
}

impl<'a> Iterator for Elements<'a> {
    type Item = Result<Element>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = match self.format {
            TreeFormat::Hg => self.next_hg(),
            TreeFormat::Git => self.next_git(),
        };

        if cfg!(debug_assertions) {
            if let Some(Ok(item)) = &item {
                let lookup = self.lookup(&item.component).unwrap();
                assert_eq!(
                    Some((item.hgid, item.flag)),
                    lookup,
                    "when lookup '{}'",
                    item.component.as_str()
                );
            }
        }

        item
    }
}

impl TryFrom<Entry> for manifest::List {
    type Error = anyhow::Error;

    fn try_from(v: Entry) -> Result<Self> {
        let mut entries = Vec::new();
        for entry in v.elements() {
            let entry = entry?;
            entries.push((
                entry.component,
                match entry.flag {
                    Flag::Directory => FsNodeMetadata::Directory(Some(entry.hgid)),
                    Flag::File(file_type) => {
                        FsNodeMetadata::File(FileMetadata::new(entry.hgid, file_type))
                    }
                },
            ))
        }
        Ok(manifest::List::Directory(entries))
    }
}

impl Element {
    pub fn new(component: PathComponentBuf, hgid: HgId, flag: Flag) -> Element {
        Element {
            component,
            hgid,
            flag,
        }
    }

    fn from_byte_slice_hg(byte_slice: &[u8]) -> Result<Element> {
        let path_len = match byte_slice.iter().position(|&x| x == b'\0') {
            Some(position) => position,
            None => return Err(format_err!("did not find path delimiter")),
        };
        let component = PathComponent::from_utf8(&byte_slice[..path_len])?.to_owned();
        if path_len + HgId::hex_len() > byte_slice.len() {
            return Err(format_err!("hgid length is shorter than expected"));
        }
        if byte_slice.len() > path_len + HgId::hex_len() + 2 {
            return Err(format_err!("entry longer than expected"));
        }
        let hex_slice = &byte_slice[path_len + 1..path_len + HgId::hex_len() + 1];
        let hgid = HgId::from_hex(hex_slice)?;
        let flag = parse_hg_flag(byte_slice.get(path_len + HgId::hex_len() + 1))?;
        let element = Element {
            component,
            hgid,
            flag,
        };
        Ok(element)
    }

    fn to_byte_vec_hg(&self) -> Vec<u8> {
        let component = self.component.as_byte_slice();
        // TODO: benchmark taking a buffer as a parameter
        // We may not use the last byte but it doesn't hurt to allocate
        let mut buffer = Vec::with_capacity(component.len() + HgId::hex_len() + 2);
        buffer.extend_from_slice(component);
        buffer.push(0);
        buffer.extend_from_slice(self.hgid.to_hex().as_ref());
        let flag = match self.flag {
            Flag::File(FileType::Regular) => None,
            Flag::File(FileType::Executable) => Some(b'x'),
            Flag::File(FileType::Symlink) => Some(b'l'),
            Flag::Directory => Some(b't'),
            Flag::File(FileType::GitSubmodule) => {
                panic!("bug: hg tree does not support git submodule")
            }
        };
        if let Some(byte) = flag {
            buffer.push(byte);
        }
        buffer
    }

    fn to_byte_vec_git(&self) -> Vec<u8> {
        let mode: &[u8] = match self.flag {
            Flag::File(FileType::Regular) => b"100644",
            Flag::File(FileType::Executable) => b"100755",
            Flag::File(FileType::Symlink) => b"120000",
            Flag::Directory => b"40000",
            Flag::File(FileType::GitSubmodule) => b"160000",
        };
        let component = self.component.as_byte_slice();
        let mut buffer = Vec::with_capacity(mode.len() + component.len() + HgId::len() + 2);
        buffer.extend_from_slice(mode);
        buffer.push(b' ');
        buffer.extend_from_slice(component);
        buffer.push(b'\0');
        buffer.extend_from_slice(self.hgid.as_ref());
        buffer
    }
}

/// Find the byte offsets used in a git entry.
/// Return (mode_len, name_len).
fn find_git_entry_positions(slice: &[u8]) -> Option<(usize, usize)> {
    // MODE ' '       NAME          '\0'                     BIN_SHA1
    //      ^         ^             ^                        ^
    //      mode_len  mode_len + 1  mode_len + 1 + name_len  .. + 1
    let mode_len = match slice.iter().position(|&x| x == b' ') {
        Some(position) => position,
        None => return None,
    };
    let name_len = match slice[mode_len..].iter().position(|&x| x == b'\0') {
        Some(position) if position > 1 => position - 1,
        _ => return None,
    };
    Some((mode_len, name_len))
}

/// Convert the git mode (ex. b"100644") to `Flag`.
fn parse_git_mode(mode: &[u8]) -> Result<Flag> {
    let flag = match mode {
        b"40000" => Flag::Directory,
        // 100664 is non-standard but present in old repos.
        // See https://github.com/git/git/commit/42ea9cb286423c949d42ad33823a5221182f84bf
        b"100644" | b"100664" => Flag::File(FileType::Regular),
        b"100755" => Flag::File(FileType::Executable),
        b"120000" => Flag::File(FileType::Symlink),
        b"160000" => Flag::File(FileType::GitSubmodule),
        s => {
            return Err(format_err!(
                "unknown or unsupported mode in git tree ({})",
                String::from_utf8_lossy(s)
            ));
        }
    };
    Ok(flag)
}

/// Convert hg flag to `Flag`.
fn parse_hg_flag(flag_byte: Option<&u8>) -> Result<Flag> {
    let flag = match flag_byte {
        Some(b'\n') | None => Flag::File(FileType::Regular),
        Some(b'x') => Flag::File(FileType::Executable),
        Some(b'l') => Flag::File(FileType::Symlink),
        Some(b't') => Flag::Directory,
        Some(bad_flag) => return Err(format_err!("invalid flag {}", bad_flag)),
    };
    Ok(flag)
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;
    use types::testutil::*;

    use super::*;

    #[test]
    fn test_element_from_byte_slice_hg() {
        let mut buffer = vec![];
        let path = PathComponent::from_str("foo").unwrap();
        let hgid = hgid("123");
        assert!(Element::from_byte_slice_hg(&buffer).is_err());
        buffer.extend_from_slice(path.as_byte_slice());
        assert!(Element::from_byte_slice_hg(&buffer).is_err());
        buffer.push(b'\0');
        assert!(Element::from_byte_slice_hg(&buffer).is_err());
        buffer.extend_from_slice(hgid.to_hex().as_ref());
        assert_eq!(
            Element::from_byte_slice_hg(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Regular))
        );

        buffer.push(b'x');
        assert_eq!(
            Element::from_byte_slice_hg(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Executable))
        );

        *buffer.last_mut().unwrap() = b'l';
        assert_eq!(
            Element::from_byte_slice_hg(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::File(FileType::Symlink))
        );

        *buffer.last_mut().unwrap() = b't';
        assert_eq!(
            Element::from_byte_slice_hg(&buffer).unwrap(),
            Element::new(path.to_owned(), hgid, Flag::Directory)
        );

        *buffer.last_mut().unwrap() = b's';
        assert!(Element::from_byte_slice_hg(&buffer).is_err());

        *buffer.last_mut().unwrap() = b'x';
        buffer.push(b'\0');
        assert!(Element::from_byte_slice_hg(&buffer).is_err());
    }

    #[test]
    fn test_deserialize_git_tree() {
        // Tree data generated by:
        //
        // mkdir -p foo && cd foo
        // ( git init; touch normal; mkdir dir; touch dir/1; ln -s normal symlink; touch exe; chmod +x exe; git add .; git commit -m c; ) >/dev/null
        // TREE=$(git cat-file -p HEAD | grep tree | sed 's/.* //')
        // git cat-file tree $TREE
        let data = b"40000 dir\x00\x8d\xc8w\xa9\x98\xd8\xc6\x1f\x90\x0e\x8bN\xe9\xb5\x01\xfa\n\x03\x93X100755 exe\x00\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91100644 normal\x00\xe6\x9d\xe2\x9b\xb2\xd1\xd6CK\x8b)\xaewZ\xd8\xc2\xe4\x8cS\x91120000 symlink\x00Z\xe04cN\x8d8,\x86F\x06\x8b\x8bR8\x18\x15\xed\xab\xf0";
        let entry = Entry(Bytes::copy_from_slice(data), TreeFormat::Git);
        let elements = entry.elements();
        let elements_str = elements
            .map(|e| format!("{:?}", e.unwrap()))
            .collect::<Vec<_>>();
        assert_eq!(
            elements_str,
            [
                "Element { component: PathComponentBuf(\"dir\"), hgid: HgId(\"8dc877a998d8c61f900e8b4ee9b501fa0a039358\"), flag: Directory }",
                "Element { component: PathComponentBuf(\"exe\"), hgid: HgId(\"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\"), flag: File(Executable) }",
                "Element { component: PathComponentBuf(\"normal\"), hgid: HgId(\"e69de29bb2d1d6434b8b29ae775ad8c2e48c5391\"), flag: File(Regular) }",
                "Element { component: PathComponentBuf(\"symlink\"), hgid: HgId(\"5ae034634e8d382c8646068b8b52381815edabf0\"), flag: File(Symlink) }",
            ]
        );
    }

    #[test]
    fn test_lookup_order() {
        let elements = vec![
            element("b-"),
            element("b0"),
            element("b/"),
            element("a"),
            element("c"),
        ];
        for format in [TreeFormat::Git, TreeFormat::Hg] {
            let entry = Entry::from_elements(elements.clone(), format);
            // Exercise the `assert_eq!` about `lookup` in `next()`.
            let _ = entry.elements().collect::<Vec<_>>();
        }
    }

    #[test]
    fn test_roundtrip_serialization_on_directory() {
        let component = PathComponentBuf::from_string(String::from("c")).unwrap();
        let hgid = HgId::from_hex(b"2e31d52f551e445002a6e6690700ce2ac31f196e").unwrap();
        let flag = Flag::Directory;
        let byte_slice = b"c\02e31d52f551e445002a6e6690700ce2ac31f196et";
        let element = Element::new(component, hgid, flag);
        assert_eq!(Element::from_byte_slice_hg(byte_slice).unwrap(), element);
        let buffer = element.to_byte_vec_hg();
        assert_eq!(buffer.to_vec(), byte_slice.to_vec());
    }

    quickcheck! {
        fn test_rountrip_serialization(
            component: PathComponentBuf,
            hgid: HgId,
            flag_proxy: Option<FileType>
        ) -> bool {
            let flag = match flag_proxy {
                Some(file_type) => Flag::File(file_type),
                None => Flag::Directory,
            };
            let element = Element::new(component, hgid, flag);
            let buffer = element.to_byte_vec_hg();
            Element::from_byte_slice_hg(&buffer).unwrap() == element
        }
    }

    fn element(name: &str) -> Element {
        let (name, flag) = crate::namecmp::tests::get_name_flag(name);
        Element::new(name, HgId::default(), flag)
    }
}
