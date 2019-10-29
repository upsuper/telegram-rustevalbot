use once_cell::sync::Lazy;

static SEEKER: Lazy<RustDocSeeker> = Lazy::new(|| {
    let data = fs::read_to_string("search-index.js").expect("cannot find search-index.js");
    let doc: RustDoc = data.parse().expect("cannot parse search-index.js");
    if cfg!(debug_assertions) {
        const SPECIAL_CHARS: &[char] = &['<', '>', '"', '\'', '&'];
        for item in doc.iter() {
            // If there is any HTML special character in item path,
            // we need to properly escape them in DocItemExt::write_item.
            if item.path.contains(SPECIAL_CHARS)
                || item
                    .parent
                    .as_ref()
                    .map_or(false, |p| p.as_ref().contains(SPECIAL_CHARS))
                || item.name.as_ref().contains(SPECIAL_CHARS)
            {
                panic!("Found path with HTML special character: {:?}", item);
            }
        }
    }
    doc.build()
});
