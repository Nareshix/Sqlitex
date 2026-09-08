/// hash function for generating checksums
pub(crate) fn fnv1a_hash(s: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash as i64
}