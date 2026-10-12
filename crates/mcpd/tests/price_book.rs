//! The join between what the cost page says about price books and what the
//! daemon can do.
//!
//! `AppState::with_price_book` is a public method with no caller. The cost page
//! told a reader to pass a book to it, which reads as a capability mcpd has: a
//! deployment can be given negotiated rates. It cannot. No flag, no config file
//! key and no endpoint reaches that method, so every deployment prices with
//! `builtin_book` and a model it does not name stays unpriced.
//!
//! The page is now honest about that. This test is what keeps it honest in both
//! directions: it fails while the page names the injection point and no code
//! path reaches it, and it goes quiet the day somebody wires one up.

use std::path::{Path, PathBuf};

/// The repository root, from this crate's own directory.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(relative: &str) -> String {
    let path = root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Whether any code the daemon binary is built from installs a price book of
/// its own.
///
/// The daemon crate rather than the whole tree: a test fixture calling the
/// method proves nothing about what an operator can do, and `mcpd-server` is a
/// library, so a caller there could be for an embedder. `crates/mcpd` is the
/// binary an operator runs.
fn the_daemon_can_load_a_book() -> bool {
    let src = root().join("crates/mcpd/src");
    let entries = std::fs::read_dir(&src).unwrap_or_else(|e| panic!("read {}: {e}", src.display()));
    entries.filter_map(Result::ok).filter(|e| e.path().extension().is_some_and(|x| x == "rs")).any(
        |e| std::fs::read_to_string(e.path()).is_ok_and(|text| text.contains("with_price_book")),
    )
}

#[test]
fn the_cost_page_names_the_price_book_injection_point_only_if_the_daemon_reaches_it() {
    // English only. The eight translations are updated from this page, and a
    // gate that failed on a stale translation would be a gate that blocks an
    // English correction until nine files move together.
    let page = read("web/src/pages/docs/cost.astro");
    if the_daemon_can_load_a_book() {
        return;
    }
    assert!(
        !page.contains("with_price_book"),
        "web/src/pages/docs/cost.astro sends the reader to AppState::with_price_book, and \
         nothing under crates/mcpd/src calls it, so no operator can reach it. Either wire a \
         flag or a config file key through to it, or say on the page that a book cannot be \
         supplied."
    );
}

#[test]
fn the_cost_page_says_a_book_cannot_be_supplied_while_that_is_true() {
    // The other half. Removing the false sentence and saying nothing would
    // leave a reader to assume the built-in table is the only one because it is
    // good enough, rather than because it is the only one there is.
    if the_daemon_can_load_a_book() {
        return;
    }
    let page = read("web/src/pages/docs/cost.astro");
    assert!(
        page.contains("You cannot supply your own book."),
        "web/src/pages/docs/cost.astro no longer tells the reader that mcpd runs the built-in \
         price book and cannot be given another"
    );
}
