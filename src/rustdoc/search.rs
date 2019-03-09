use fst_subseq_ascii_caseless::SubseqAsciiCaseless;
use lazy_static::lazy_static;
use matches::matches;
use rustdoc_seeker::{DocItem, RustDoc, RustDocSeeker, TypeItem};
use std::fs;
use std::ops::Deref;

lazy_static! {
    static ref SEEKER: RustDocSeeker = {
        let data = fs::read_to_string("search-index.js").expect("cannot find search-index.js");
        let doc: RustDoc = data.parse().expect("cannot parse search-index.js");
        if cfg!(debug_assertions) {
            const SPECIAL_CHARS: &[char] = &['<', '>', '"', '\'', '&'];
            for item in doc.iter() {
                // If there is any HTML special character in item path,
                // we need to properly escape them in DocItemExt::write_item.
                if item.path.contains(SPECIAL_CHARS) ||
                    item.parent.as_ref().map_or(false, |p| p.as_ref().contains(SPECIAL_CHARS)) ||
                    item.name.as_ref().contains(SPECIAL_CHARS) {
                    panic!("Found path with HTML special character: {:?}", item);
                }
            }
        }
        doc.build()
    };
}

pub fn init() {
    lazy_static::initialize(&SEEKER);
}

pub fn query(path: &str) -> Vec<&'static DocItem> {
    let path = path
        .split("::")
        .map(|s| s.trim_matches(char::is_whitespace))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let QueryPath { root, path, name } = match split_path(&path) {
        Some(query) => query,
        None => return vec![],
    };
    let lowercase_name = name.to_ascii_lowercase();
    let mut matched_items = SEEKER
        .search(&SubseqAsciiCaseless::new(&lowercase_name))
        .filter(|item| matches_path(item, root, path))
        .collect::<Vec<_>>();
    if matched_items.is_empty() {
        return vec![];
    }
    // Sort items.
    matched_items.sort_by_key(|item| {
        (
            item.name.as_ref().len(),
            // Prefer items with description.
            item.desc.is_empty(),
            ItemType::from(&item.name),
            &item.path,
            item.parent.as_ref().map(|p| p.as_ref()),
        )
    });
    matched_items
}

struct QueryPath<'a> {
    root: RootLevel,
    path: &'a [&'a str],
    name: &'a str,
}

fn split_path<'a>(path: &'a [&'a str]) -> Option<QueryPath<'a>> {
    let (root, path) = match path.split_first()? {
        (root, p) => match RootLevel::from_str(root) {
            Some(r) => (r, p),
            None => (RootLevel::Std, path),
        },
    };
    let (name, path) = path.split_last()?;
    Some(QueryPath { root, path, name })
}

macro_rules! define_enum {
    (
        enum $name:ident {
            $($variant:ident => $str:expr,)+
        }
    ) => {
        #[derive(Clone, Copy)]
        enum $name {
            $($variant,)+
        }

        impl $name {
            fn from_str(s: &str) -> Option<Self> {
                Some(match s {
                    $($str => $name::$variant,)+
                    _ => return None,
                })
            }

            fn as_str(&self) -> &'static str {
                match self {
                    $($name::$variant => $str,)+
                }
            }
        }
    }
}

define_enum! {
    enum RootLevel {
        Alloc => "alloc",
        Core => "core",
        Std => "std",
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
pub enum ItemType {
    Keyword,
    Primitive,
    Macro,
    Other,
}

impl<'a> From<&'a TypeItem> for ItemType {
    fn from(item: &TypeItem) -> Self {
        match item {
            TypeItem::Keyword(_) => ItemType::Keyword,
            TypeItem::Primitive(_) => ItemType::Primitive,
            TypeItem::Macro(_) => ItemType::Macro,
            _ => ItemType::Other,
        }
    }
}

impl ItemType {
    pub fn is_keyword_or_primitive(&self) -> bool {
        matches!(self, ItemType::Keyword | ItemType::Primitive)
    }

    pub fn is_macro(&self) -> bool {
        matches!(self, ItemType::Macro)
    }
}

fn matches_path(item: &DocItem, root: RootLevel, path: &[&str]) -> bool {
    let mut item_path = item
        .path
        .split("::")
        .chain(item.parent.iter().map(|p| p.as_ref().deref()));
    // Each level in the query path should be found in the item path
    // with the same order.
    item_path.next().unwrap() == root.as_str()
        && path
            .iter()
            .all(|level| item_path.any(|l| l.contains(level)))
}

#[cfg(test)]
mod test {
    use super::*;
    use rustdoc_seeker::DocItem;
    use string_cache::DefaultAtom as Atom;

    #[test]
    fn test_matches_path() {
        let item = DocItem::new(
            TypeItem::Method(Atom::from("eq")),
            Some(TypeItem::Struct(Atom::from("BTreeMap"))),
            Atom::from("std::collections"),
            Atom::from(""),
        );
        assert!(matches_path(&item, RootLevel::Std, &["BTreeMap"]));
        assert!(matches_path(&item, RootLevel::Std, &["col"]));
        assert!(matches_path(&item, RootLevel::Std, &["Map"]));
        assert!(matches_path(&item, RootLevel::Std, &["col", "Map"]));
        // XXX We may want to support case-insensitive matching
        assert!(!matches_path(&item, RootLevel::Std, &["map"]));
        // XXX We may want to support fuzzy matching
        assert!(!matches_path(&item, RootLevel::Std, &["BMap"]));
        assert!(!matches_path(&item, RootLevel::Std, &["x"]));
        assert!(!matches_path(&item, RootLevel::Alloc, &["BTreeMap"]));
    }
}
