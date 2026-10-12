//! The join between what the cost page says about price books and what the
//! daemon can do.
//!
//! The cost page tells a reader to load their own book with `--price-book`
//! and to refresh rates with `mcpd prices refresh`. Both are only true while
//! the daemon binary installs a loaded book and the flag exists, so this test
//! fails the day either half goes and the page goes on promising it.

use std::path::{Path, PathBuf};

/// The repository root, from this crate's own directory.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

#[test]
fn the_cost_page_promises_a_loadable_book_only_while_the_daemon_loads_one() {
    let main = read("crates/mcpd/src/main.rs");
    let page = read("web/src/pages/docs/cost.astro");
    assert!(
        main.contains("with_price_book") && main.contains("price_book: Option<String>"),
        "crates/mcpd/src/main.rs no longer installs a book from --price-book, and \
         web/src/pages/docs/cost.astro tells readers to load one"
    );
    assert!(page.contains("--price-book"), "the cost page no longer says how to load a book");
    assert!(
        page.contains("mcpd prices refresh"),
        "the cost page no longer says how to refresh the rates"
    );
}

#[test]
fn the_builtin_book_is_the_file_the_refresh_command_writes() {
    // A hand edit to the JSON would survive until the next refresh silently
    // reverted it, so the file must be exactly what the writer produces.
    let text = read("crates/mcpd-core/prices/builtin.json");
    let book = mcpd_core::cost::PriceBook::from_json(&text).expect("builtin.json loads");
    assert_eq!(
        mcpd_core::price_catalogue::to_json(&book),
        text,
        "crates/mcpd-core/prices/builtin.json was edited by hand; run `make prices`, or \
         `mcpd prices refresh --out crates/mcpd-core/prices/builtin.json` with the same --source"
    );
}
