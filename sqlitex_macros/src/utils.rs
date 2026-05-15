use sqlformat::{FormatOptions, Indent, QueryParams, format};

/// hash function for generating checksums
pub(crate) fn fnv1a_hash(s: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}

/// This nicely formats the sql string.///
/// Useful for vscode hover over fn
pub(crate) fn format_sql(sql: &str) -> String {
    let options = FormatOptions {
        indent: Indent::Tabs,
        ..Default::default()
    };
    format(sql, &QueryParams::None, &options)
}
