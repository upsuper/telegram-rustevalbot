use super::{CommandImpl, ExecutionContext};
use crate::utils::{self, WidthCountingWriter};
use fst_subseq_ascii_caseless::SubseqAsciiCaseless;
use futures::{Future, IntoFuture};
use htmlescape::encode_minimal;
use lazy_static::lazy_static;
use matches::matches;
use rustdoc_seeker::{DocItem, RustDoc, RustDocSeeker, TypeItem};
use std::fmt::{self, Write};
use std::fs;
use std::ops::Deref;
use unicode_width::UnicodeWidthStr;

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

pub struct DocCommand;

impl CommandImpl for DocCommand {
    type Flags = ();

    fn run(
        ctx: &ExecutionContext,
        _flags: &(),
        arg: &str,
    ) -> Box<dyn Future<Item = String, Error = &'static str>> {
        let path = arg
            .split("::")
            .map(|s| s.trim_matches(char::is_whitespace))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        let QueryPath { root, path, name } = match split_path(&path) {
            Some(query) => query,
            None => return Box::new(Ok("(empty query)".to_string()).into_future()),
        };
        let lowercase_name = name.to_ascii_lowercase();
        let mut matched_items = SEEKER
            .search(&SubseqAsciiCaseless::new(&lowercase_name))
            .filter(|item| item.matches_path(root, path))
            .collect::<Vec<_>>();
        if matched_items.is_empty() {
            return Box::new(Ok("(empty result)".to_string()).into_future());
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
        // Return only limited number of results.
        let max_count = if ctx.is_private { 10 } else { 3 };
        matched_items.truncate(max_count);
        // Generate result.
        let mut result = String::new();
        for item in &matched_items {
            item.write_item(&mut result).unwrap();
            result.push('\n');
        }
        Box::new(Ok(result).into_future())
    }
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
enum ItemType {
    Keyword,
    Primitive,
    Other,
}

impl<'a> From<&'a TypeItem> for ItemType {
    fn from(item: &TypeItem) -> Self {
        match item {
            TypeItem::Keyword(_) => ItemType::Keyword,
            TypeItem::Primitive(_) => ItemType::Primitive,
            _ => ItemType::Other,
        }
    }
}

impl ItemType {
    fn is_keyword_or_primitive(&self) -> bool {
        matches!(self, ItemType::Keyword | ItemType::Primitive)
    }
}

trait DocItemExt {
    fn matches_path(&self, root: RootLevel, path: &[&str]) -> bool;
    fn write_item(&self, output: &mut impl fmt::Write) -> fmt::Result;
}

impl DocItemExt for DocItem {
    fn matches_path(&self, root: RootLevel, path: &[&str]) -> bool {
        let mut item_path = self
            .path
            .split("::")
            .chain(self.parent.iter().map(|p| p.as_ref().deref()));
        // Each level in the query path should be found in the item path
        // with the same order.
        item_path.next().unwrap() == root.as_str() && path
            .iter()
            .all(|level| item_path.any(|l| l.contains(level)))
    }

    fn write_item(&self, mut output: &mut impl fmt::Write) -> fmt::Result {
        // Write link tag.
        output.write_str(r#"<a href="https://doc.rust-lang.org/"#)?;
        self.fmt_url(output)?;
        output.write_str(r#"">"#)?;
        // Write full path. We don't escape them and we pass them into
        // WidthCountingWriter directly assuming they don't contain any
        // HTML special characters. This is checked in debug assertions
        // in the lazy_static block above.
        let ty = ItemType::from(&self.name);
        let path_width = {
            let mut output = WidthCountingWriter::new(&mut output);
            if !ty.is_keyword_or_primitive() {
                let is_parent_keyword_or_primitive = self
                    .parent
                    .as_ref()
                    .map_or(false, |p| ItemType::from(p).is_keyword_or_primitive());
                if !is_parent_keyword_or_primitive {
                    write!(output, "{}::", self.path)?;
                }
            }
            if let Some(parent) = &self.parent {
                write!(output, "{}::", parent.as_ref())?;
            }
            output.write_str(self.name.as_ref())?;
            if matches!(self.name, TypeItem::Macro(_)) {
                output.write_char('!')?;
            }
            output.total_width()
        };
        output.write_str("</a>")?;
        let type_str = match ty {
            ItemType::Keyword => " (keyword)",
            ItemType::Primitive => " (primitive type)",
            _ => "",
        };
        output.write_str(type_str)?;
        // Write description.
        const TOTAL_MAX_WIDTH: usize = 80;
        const DESC_SEP: &str = " - ";
        let written_width = path_width + type_str.width_cjk();
        let remaining_len = TOTAL_MAX_WIDTH.checked_sub(written_width + DESC_SEP.len());
        if !self.desc.is_empty() && remaining_len.is_some() {
            let remaining_len = remaining_len.unwrap();
            output.write_str(DESC_SEP)?;
            let desc = utils::truncate_output(&self.desc, 1, remaining_len);
            output.write_str(&encode_minimal(&desc))?;
        }
        Ok(())
    }
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
        assert!(item.matches_path(RootLevel::Std, &["BTreeMap"]));
        assert!(item.matches_path(RootLevel::Std, &["col"]));
        assert!(item.matches_path(RootLevel::Std, &["Map"]));
        assert!(item.matches_path(RootLevel::Std, &["col", "Map"]));
        // XXX We may want to support case-insensitive matching
        assert!(!item.matches_path(RootLevel::Std, &["map"]));
        // XXX We may want to support fuzzy matching
        assert!(!item.matches_path(RootLevel::Std, &["BMap"]));
        assert!(!item.matches_path(RootLevel::Std, &["x"]));
        assert!(!item.matches_path(RootLevel::Alloc, &["BTreeMap"]));
    }
}
