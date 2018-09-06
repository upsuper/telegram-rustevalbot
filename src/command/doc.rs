use fst::automaton::Subsequence;
use futures::{Future, IntoFuture};
use htmlescape::encode_minimal;
use lazy_static;
use rustdoc_seeker::{DocItem, RustDoc, RustDocSeeker, TypeItem};
use std::cmp;
use std::fmt;
use std::fs;
use std::ops::Deref;

use super::ExecutionContext;
use utils;

lazy_static! {
    static ref SEEKER: RustDocSeeker = {
        let data = fs::read_to_string("search-index.js").expect("cannot find search-index.js");
        let doc: RustDoc = data.parse().expect("cannot parse search-index.js");
        doc.build().unwrap()
    };
}

pub(super) fn init() {
    lazy_static::initialize(&SEEKER);
}

pub(super) fn run(ctx: ExecutionContext) -> impl Future<Item = String, Error = &'static str> {
    let path = ctx
        .args
        .split("::")
        .map(|s| s.trim_matches(utils::is_separator))
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    let QueryPath { root, path, name } = match split_path(&path) {
        Some(query) => query,
        None => return Ok("(empty query)".to_string()).into_future(),
    };
    let mut matched_items = SEEKER
        .search(&Subsequence::new(name))
        .filter(|item| item.matches_path(root, path))
        .collect::<Vec<_>>();
    if matched_items.is_empty() {
        return Ok("(empty result)".to_string()).into_future();
    }
    matched_items.sort_by_key(|item| {
        (
            item.name.as_ref().len(),
            !is_primitive(&item.name),
            &item.path,
            item.parent.as_ref().map(|p| p.as_ref()),
        )
    });
    // Return only limited number of results.
    let max_count = if ctx.is_private { 10 } else { 3 };
    let count = cmp::min(matched_items.len(), max_count);
    let matched_items = &matched_items[..count];
    // Generate result.
    let mut result = String::new();
    for item in matched_items.iter() {
        item.write_item(&mut result).unwrap();
        result.push('\n');
    }
    return Ok(result).into_future();
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

fn is_primitive(ti: &TypeItem) -> bool {
    matches!(ti, TypeItem::Primitive(_))
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
        if item_path.next().unwrap() != root.as_str() {
            return false;
        }
        for level in path.iter() {
            loop {
                match item_path.next() {
                    Some(l) => {
                        if l.contains(level) {
                            break;
                        }
                    }
                    None => return false,
                }
            }
        }
        true
    }

    fn write_item(&self, output: &mut impl fmt::Write) -> fmt::Result {
        // Write link tag.
        output.write_str(r#"<a href="https://doc.rust-lang.org/"#)?;
        write!(output, "{}", self)?;
        output.write_str(r#"">"#)?;
        // Write full path.
        if !is_primitive(&self.name)
            && !self.parent.as_ref().map_or(false, is_primitive) {
            write!(output, "{}::", self.path)?;
        }
        if let Some(parent) = &self.parent {
            write!(output, "{}::", parent.as_ref())?;
        }
        output.write_str(self.name.as_ref())?;
        if matches!(self.name, TypeItem::Macro(_)) {
            output.write_char('!')?;
        }
        output.write_str("</a>")?;
        if is_primitive(&self.name) {
            output.write_str(" (primitive type)")?;
        }
        // Write description.
        if !self.desc.is_empty() {
            output.write_str(" - ")?;
            const MAX_LEN: usize = 50;
            if self.desc.len() > MAX_LEN {
                // This assumes that we don't have non-ASCII character
                // in descriptions.
                output.write_str(&encode_minimal(&self.desc[..MAX_LEN - 3]))?;
                output.write_str("...")?;
            } else {
                output.write_str(&encode_minimal(&self.desc))?;
            }
        }
        Ok(())
    }
}
