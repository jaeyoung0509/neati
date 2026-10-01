//! Positive generated-subtree formats, separate from deployment output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkGeneratedKind {
    SvelteKitTypes,
    NextWebpackCache,
}

/// Webpack FileMiddleware's uncompressed v1 header. Parse only the bounded
/// section table; never deserialize code or follow lazy-pointer file names.
pub fn supported_webpack_pack(header: &[u8], file_len: u64) -> bool {
    if header.len() < 8 || u32::from_le_bytes(header[..4].try_into().unwrap()) != 0x01637077 {
        return false;
    }
    let count = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    if count == 0 || count > 8192 || header.len() != 8 + count * 4 {
        return false;
    }
    let mut total = header.len() as u64;
    for chunk in header[8..].as_chunks::<4>().0 {
        let value = i32::from_le_bytes(*chunk);
        if value == i32::MIN {
            return false;
        }
        let Some(next) = total.checked_add(value.unsigned_abs() as u64) else {
            return false;
        };
        total = next;
    }
    total == file_len
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_contract_requires_complete_versioned_bounded_section_table() {
        let mut bytes = Vec::new();
        for word in [0x01637077u32, 1, 10] {
            bytes.extend(word.to_le_bytes());
        }
        assert!(supported_webpack_pack(&bytes, 22));
        assert!(!supported_webpack_pack(&bytes, 21));
        bytes[0] = 0;
        assert!(!supported_webpack_pack(&bytes, 22));
        assert!(!supported_webpack_pack(b"authored content", 16));
    }
}
