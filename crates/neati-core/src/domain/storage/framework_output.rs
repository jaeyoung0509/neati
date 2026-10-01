//! Positive generated-subtree formats, separate from deployment output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameworkGeneratedKind {
    SvelteKitTypes,
    NextWebpackCache,
    SvelteKitOutput,
    NextOutput,
}

impl FrameworkGeneratedKind {
    pub fn relative(self) -> &'static str {
        match self {
            Self::SvelteKitTypes => ".svelte-kit/types",
            Self::NextWebpackCache => ".next/cache/webpack",
            Self::SvelteKitOutput => ".svelte-kit",
            Self::NextOutput => ".next",
        }
    }

    pub fn dependency(self) -> &'static str {
        match self {
            Self::SvelteKitTypes | Self::SvelteKitOutput => "@sveltejs/kit",
            Self::NextWebpackCache | Self::NextOutput => "next",
        }
    }

    pub fn is_whole(self) -> bool {
        matches!(self, Self::SvelteKitOutput | Self::NextOutput)
    }
}

/// Exactly the inert configuration subset we can establish without running JS.
/// Objects must be JSON, optionally wrapped by one fixed export assignment.
/// JS spreads, getters, imports, functions, computed keys and duplicate keys
/// are never default-output evidence.
pub fn static_framework_config(text: &str) -> Option<serde_json::Value> {
    let text = text.trim();
    let text = text
        .strip_prefix("export default ")
        .or_else(|| text.strip_prefix("module.exports = "))
        .unwrap_or(text)
        .trim_end_matches(';')
        .trim();
    if text.len() > 64 * 1024 || !text.starts_with('{') {
        return None;
    }
    // serde_json accepts duplicate keys. A tiny visitor refuses them and caps
    // nesting before deserializing into the ordinary JSON representation.
    use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
    struct Strict(usize);
    impl<'de> DeserializeSeed<'de> for Strict {
        type Value = serde_json::Value;
        fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            if self.0 > 16 {
                return Err(serde::de::Error::custom("configuration depth"));
            }
            d.deserialize_any(self)
        }
    }
    impl<'de> Visitor<'de> for Strict {
        type Value = serde_json::Value;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("bounded unique-key JSON")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut m: M) -> Result<Self::Value, M::Error> {
            let mut out = serde_json::Map::new();
            while let Some(key) = m.next_key::<String>()? {
                if out.contains_key(&key) || out.len() >= 1024 {
                    return Err(serde::de::Error::custom(
                        "duplicate or excessive config keys",
                    ));
                }
                out.insert(key, m.next_value_seed(Strict(self.0 + 1))?);
            }
            Ok(out.into())
        }
        fn visit_seq<S: SeqAccess<'de>>(self, mut s: S) -> Result<Self::Value, S::Error> {
            let mut out = Vec::new();
            while let Some(value) = s.next_element_seed(Strict(self.0 + 1))? {
                if out.len() >= 1024 {
                    return Err(serde::de::Error::custom("excessive config entries"));
                }
                out.push(value);
            }
            Ok(out.into())
        }
        fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
            Ok(v.into())
        }
        fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
            Ok(v.into())
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
            Ok(v.into())
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
            Ok(v.into())
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
            Ok(v.into())
        }
        fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
            Ok(serde_json::Value::from(v))
        }
        fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
            Ok(serde_json::Value::Null)
        }
    }
    let mut reader = serde_json::Deserializer::from_str(text);
    let value = Strict(0).deserialize(&mut reader).ok()?;
    reader.end().ok()?;
    value.is_object().then_some(value)
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

    #[test]
    fn static_config_requires_complete_inert_unique_bounded_json() {
        assert_eq!(
            static_framework_config("export default {\"kit\":{\"outDir\":\".svelte-kit\"}};"),
            Some(serde_json::json!({"kit":{"outDir":".svelte-kit"}}))
        );
        assert!(static_framework_config("module.exports = {\"distDir\":\".next\"};").is_some());
        for text in [
            "export default {...config}",
            "export default {distDir: '.next'}",
            "export default {\"distDir\":\".next\",\"distDir\":\"custom\"}",
            "export default {\"kit\":{\"outDir\":\"x\",\"outDir\":\"y\"}}",
            "export default {}; run()",
            "export default () => ({})",
            "{\"__proto__\":{}}; evil()",
        ] {
            assert!(static_framework_config(text).is_none(), "{text}");
        }
        assert!(
            static_framework_config(&format!("{}0{}", "[".repeat(20), "]".repeat(20))).is_none()
        );
        assert!(
            static_framework_config(&format!("{{\"padding\":\"{}\"}}", "x".repeat(65536)))
                .is_none()
        );
    }
}
