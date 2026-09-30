//! Positive format contract, backed by Node v26.7.0 compile_cache.{cc,h}.
//! This recognizes disposable payloads; native ownership, age, scope and use
//! evidence are still required. Other versions/layouts remain advisory.
pub const NODE_CACHE_MIN_DAYS: u64 = 3;
pub const NODE_CACHE_MAX_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const NODE_CACHE_MAX_UNIT_BYTES: u64 = 64 * 1024 * 1024;
pub const NODE_CACHE_MAX_FILES: usize = 512;

pub fn supported_node_group(name: &str, uid: u32) -> bool {
    name == format!("v26.7.0-arm64-8d7ad2ee-{uid}")
}

pub fn node_cache_file(name: &str, bytes: &[u8]) -> bool {
    if name.len() != 8
        || !name
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        || bytes.len() < 24
        || bytes.len() > NODE_CACHE_MAX_FILE_BYTES
    {
        return false;
    }
    let word = |offset| u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
    word(0) == 0x8adfdbb2
        && word(4) > 0
        && word(8) as usize == bytes.len() - 20
        && word(16) == crc32fast::hash(&bytes[20..])
        && word(20) == 0xc0de05c0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn supported_group_binds_writer_version_architecture_tag_and_user() {
        assert!(supported_node_group("v26.7.0-arm64-8d7ad2ee-501", 501));
        for name in [
            "v26.7.0-arm64-8d7ad2ee-502",
            "v26.7.0-arm64-8d7ad2ee-0501",
            "v26.7.1-arm64-8d7ad2ee-501",
            "v26.7.0-x64-8d7ad2ee-501",
            "v26.7.0-arm64-deadbeef-501",
            "node-compile-cache-project",
        ] {
            assert!(!supported_node_group(name, 501), "{name}");
        }
    }
    #[test]
    fn payload_requires_positive_header_length_and_checksum() {
        let payload = 0xc0de05c0u32.to_le_bytes();
        let mut bytes = Vec::new();
        for word in [0x8adfdbb2u32, 10, 4, 123, crc32fast::hash(&payload)] {
            bytes.extend(word.to_le_bytes());
        }
        bytes.extend(payload);
        assert!(node_cache_file("0123abcd", &bytes));
        assert!(!node_cache_file("project.js", &bytes));
        for offset in [0, 4, 8, 16, 20] {
            let mut changed = bytes.clone();
            changed[offset] ^= 1;
            // Source length is positive in both cases, unlike integrity fields.
            if offset != 4 {
                assert!(!node_cache_file("0123abcd", &changed));
            }
        }
        assert!(!node_cache_file("0123abcd", &bytes[..20]));
        bytes.extend([0]);
        assert!(!node_cache_file("0123abcd", &bytes));
    }
    #[test]
    fn actual_node_writer_fixture_and_corruption_are_distinguished() {
        let bytes = include_bytes!("../../../tests/fixtures/node-26.7.0-arm64-cache.bin");
        assert!(node_cache_file("ae127978", bytes));
        let mut corrupt = bytes.to_vec();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(!node_cache_file("ae127978", &corrupt));
        let mut empty_source = bytes.to_vec();
        empty_source[4..8].fill(0);
        assert!(!node_cache_file("ae127978", &empty_source));
    }
}
