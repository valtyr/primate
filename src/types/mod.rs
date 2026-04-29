//! Type system for primate
//!
//! Defines all supported types and their parsing/normalization.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::LazyLock;

/// All supported types in primate
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Type {
    // Scalars
    // r[impl type.scalar.i32]
    I32,
    // r[impl type.scalar.i64]
    I64,
    // r[impl type.scalar.u32]
    U32,
    // r[impl type.scalar.u64]
    U64,
    // r[impl type.scalar.f32]
    F32,
    // r[impl type.scalar.f64]
    F64,
    // r[impl type.scalar.bool]
    Bool,
    // r[impl type.scalar.string]
    String,

    // Semantic types
    // r[impl type.duration.format]
    Duration,

    // Special types
    // r[impl type.special.regex]
    Regex,
    // r[impl type.special.url]
    Url,

    // Containers
    // r[impl type.container.array]
    Array {
        // r[impl ir.type.array]
        element: Box<Type>,
    },
    /// Fixed-length homogeneous array (RFC 0003 §2). Emitted as `[T; N]` in
    /// Rust, and as a homogeneous tuple in target languages without a native
    /// fixed-array form.
    FixedArray {
        element: Box<Type>,
        length: u32,
    },
    // r[impl type.container.map]
    Map {
        // r[impl ir.type.map]
        key: Box<Type>,
        value: Box<Type>,
    },
    // r[impl type.container.tuple]
    Tuple {
        // r[impl ir.type.tuple]
        elements: Vec<Type>,
    },
    // r[impl type.container.optional]
    Optional {
        // r[impl ir.type.optional]
        inner: Box<Type>,
    },

    // Complex types
    Enum {
        name: std::string::String,
        /// Namespace the enum was declared in. Generators use this together
        /// with the namespace they're currently emitting into to decide
        /// whether to emit a bare reference or a cross-namespace import.
        /// Empty when the enum is in the same namespace as the reference,
        /// or when the type was constructed without namespace info (older
        /// JSON, tests).
        #[serde(default, skip_serializing_if = "std::string::String::is_empty")]
        namespace: std::string::String,
    },
    // r[impl type.struct.infer]
    // r[impl type.struct.explicit]
    Struct {
        // r[impl ir.type.struct]
        fields: HashMap<std::string::String, Type>,
    },
    /// A user-defined type alias reference. Generators emit this as the bare
    /// alias name; the alias declaration itself comes from `CodeGenRequest::aliases`.
    Alias {
        name: std::string::String,
        /// Namespace the alias was declared in. See `Enum.namespace`.
        #[serde(default, skip_serializing_if = "std::string::String::is_empty")]
        namespace: std::string::String,
    },
}

/// Normalized value representation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    // Scalars
    // r[impl ir.value.scalar]
    /// Integer value. Stored as `i128` so every primate primitive integer
    /// type fits without sign-cast cleverness — `u64::MAX` is `~1.8e19`,
    /// well within `i128`'s range. The accompanying `Type` carries the
    /// declared primitive (i32/i64/u32/u64), which generators use to
    /// pick the right output form.
    Integer(i128),
    Float(f64),
    Bool(bool),
    String(std::string::String),

    // Semantic types
    // r[impl ir.value.duration]
    // r[impl type.duration.internal]
    Duration { nanoseconds: u64 },

    // Containers
    Array(Vec<Value>),
    Map(HashMap<std::string::String, Value>),
    Tuple(Vec<Value>),
    Optional(Option<Box<Value>>),

    // Complex types
    // r[impl ir.value.enum]
    Enum {
        variant: std::string::String,
        value: Box<Value>,
    },
    // r[impl ir.value.struct]
    Struct(HashMap<std::string::String, Value>),
}

/// Parse a type string into a Type
pub fn parse_type(s: &str) -> Result<Type, TypeError> {
    let s = s.trim();

    // Handle simple types
    match s {
        "i32" => return Ok(Type::I32),
        "i64" => return Ok(Type::I64),
        "u32" => return Ok(Type::U32),
        "u64" => return Ok(Type::U64),
        "f32" => return Ok(Type::F32),
        "f64" => return Ok(Type::F64),
        "bool" => return Ok(Type::Bool),
        "string" => return Ok(Type::String),
        "duration" => return Ok(Type::Duration),
        "regex" => return Ok(Type::Regex),
        "url" => return Ok(Type::Url),
        "enum" => return Ok(Type::Enum { name: std::string::String::new(), namespace: std::string::String::new() }),
        // r[impl type.struct.infer]
        // Struct type - fields will be inferred from value or explicit fields attribute
        "struct" => return Ok(Type::Struct { fields: HashMap::new() }),
        _ => {}
    }

    // Handle array syntax: T[]
    if let Some(inner) = s.strip_suffix("[]") {
        return Ok(Type::Array {
            element: Box::new(parse_type(inner)?),
        });
    }

    // Handle optional<T>
    if let Some(inner) = s.strip_prefix("optional<").and_then(|s| s.strip_suffix('>')) {
        return Ok(Type::Optional {
            inner: Box::new(parse_type(inner)?),
        });
    }

    // Handle map<K,V>
    if let Some(inner) = s.strip_prefix("map<").and_then(|s| s.strip_suffix('>')) {
        if let Some((k, v)) = inner.split_once(',') {
            return Ok(Type::Map {
                key: Box::new(parse_type(k.trim())?),
                value: Box::new(parse_type(v.trim())?),
            });
        }
    }

    // Handle tuple<T1,T2,...>
    if let Some(inner) = s.strip_prefix("tuple<").and_then(|s| s.strip_suffix('>')) {
        let elements: Result<Vec<_>, _> = inner
            .split(',')
            .map(|t| parse_type(t.trim()))
            .collect();
        return Ok(Type::Tuple { elements: elements? });
    }

    // r[impl diag.error.unknown-type]
    Err(TypeError::Unknown(s.to_string()))
}

// r[impl type.duration.format]
static DURATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:(\d+)d)?(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?(?:(\d+)ms)?$").unwrap()
});

/// Parse a duration string into nanoseconds
/// Formats: 150ms, 5s, 3m, 2h, 1h30m, 2d
pub fn parse_duration(s: &str) -> Result<u64, ValueError> {
    let s = s.trim();

    if let Some(caps) = DURATION_RE.captures(s) {
        let days: u64 = caps.get(1).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let hours: u64 = caps.get(2).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let minutes: u64 = caps.get(3).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let seconds: u64 = caps.get(4).map_or(0, |m| m.as_str().parse().unwrap_or(0));
        let millis: u64 = caps.get(5).map_or(0, |m| m.as_str().parse().unwrap_or(0));

        if days == 0 && hours == 0 && minutes == 0 && seconds == 0 && millis == 0 {
            return Err(ValueError::InvalidDuration(s.to_string()));
        }

        let nanos = days * 24 * 60 * 60 * 1_000_000_000
            + hours * 60 * 60 * 1_000_000_000
            + minutes * 60 * 1_000_000_000
            + seconds * 1_000_000_000
            + millis * 1_000_000;

        Ok(nanos)
    } else {
        Err(ValueError::InvalidDuration(s.to_string()))
    }
}

// r[impl type.bytes.format]
static BYTES_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d+(?:\.\d+)?)\s*(B|KB|MB|GB|TB|KiB|MiB|GiB|TiB)?$").unwrap()
});

/// Parse a byte size string into bytes
/// Formats: KB, MB, GB, TB, KiB, MiB, GiB, TiB
pub fn parse_bytes(s: &str) -> Result<u64, ValueError> {
    let s = s.trim();

    if let Some(caps) = BYTES_RE.captures(s) {
        let value: f64 = caps.get(1).unwrap().as_str().parse().unwrap();
        let unit = caps.get(2).map_or("B", |m| m.as_str());

        let multiplier: u64 = match unit {
            "B" => 1,
            "KB" => 1_000,
            "MB" => 1_000_000,
            "GB" => 1_000_000_000,
            "TB" => 1_000_000_000_000,
            "KiB" => 1_024,
            "MiB" => 1_024 * 1_024,
            "GiB" => 1_024 * 1_024 * 1_024,
            "TiB" => 1_024 * 1_024 * 1_024 * 1_024,
            _ => return Err(ValueError::InvalidBytes(s.to_string())),
        };

        let bytes = (value * multiplier as f64) as u64;
        Ok(bytes)
    } else {
        Err(ValueError::InvalidBytes(s.to_string()))
    }
}

/// Validate a regex pattern
pub fn validate_regex(pattern: &str) -> Result<(), ValueError> {
    Regex::new(pattern).map_err(|e| ValueError::InvalidRegex(e.to_string()))?;
    Ok(())
}

/// Validate a URL
pub fn validate_url(url: &str) -> Result<(), ValueError> {
    // Basic URL validation - starts with http:// or https://
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(())
    } else {
        Err(ValueError::InvalidUrl(url.to_string()))
    }
}

/// Check if a name is SCREAMING_SNAKE_CASE
// r[impl input.constant.naming]
// r[impl naming.input]
pub fn is_screaming_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let mut chars = s.chars().peekable();
    let first = chars.next().unwrap();

    // Must start with uppercase letter
    if !first.is_ascii_uppercase() {
        return false;
    }

    for c in chars {
        if !c.is_ascii_uppercase() && !c.is_ascii_digit() && c != '_' {
            return false;
        }
    }

    // No double underscores, no leading/trailing underscores
    !s.contains("__") && !s.starts_with('_') && !s.ends_with('_')
}

/// Convert SCREAMING_SNAKE_CASE to camelCase
// r[impl naming.ts]
pub fn to_camel_case(s: &str) -> std::string::String {
    let mut result = std::string::String::new();
    let mut capitalize_next = false;

    for (i, c) in s.chars().enumerate() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else if i == 0 {
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }

    result
}

/// Convert string to PascalCase (for enum variants)
pub fn to_pascal_case(s: &str) -> std::string::String {
    // If the input has no word separators, treat it as already-cased and
    // preserve it as-is (just capitalizing the first character if needed).
    // This avoids mangling identifiers like `LogLevel` into `Loglevel` —
    // which the parser already enforces is PascalCase via the
    // naming-convention diagnostic.
    if !s.contains('_') && !s.contains('-') && !s.contains(' ') {
        let mut chars = s.chars();
        return match chars.next() {
            None => std::string::String::new(),
            Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        };
    }

    let mut result = std::string::String::new();
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' || c == '-' || c == ' ' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c.to_ascii_lowercase());
        }
    }

    result
}

/// TypeScript reserved keywords
// r[impl naming.keyword-escape]
pub const TS_KEYWORDS: &[&str] = &[
    "break", "case", "catch", "class", "const", "continue", "debugger", "default",
    "delete", "do", "else", "enum", "export", "extends", "false", "finally", "for",
    "function", "if", "import", "in", "instanceof", "new", "null", "return", "super",
    "switch", "this", "throw", "true", "try", "typeof", "var", "void", "while", "with",
    "as", "implements", "interface", "let", "package", "private", "protected", "public",
    "static", "yield", "type",
];

/// Escape a name if it's a reserved keyword
pub fn escape_keyword(name: &str, keywords: &[&str]) -> std::string::String {
    if keywords.contains(&name) {
        format!("{}_", name)
    } else {
        name.to_string()
    }
}

/// Resolve a `Type::Alias` to its underlying type via the given lookup.
/// Recurses through containers and through alias chains.
pub fn resolve_alias(typ: &Type, aliases: &HashMap<std::string::String, Type>) -> Type {
    match typ {
        Type::Alias { name, .. } => match aliases.get(name) {
            Some(t) => resolve_alias(t, aliases),
            None => typ.clone(),
        },
        Type::Array { element } => Type::Array {
            element: Box::new(resolve_alias(element, aliases)),
        },
        Type::Optional { inner } => Type::Optional {
            inner: Box::new(resolve_alias(inner, aliases)),
        },
        Type::Map { key, value } => Type::Map {
            key: Box::new(resolve_alias(key, aliases)),
            value: Box::new(resolve_alias(value, aliases)),
        },
        Type::Tuple { elements } => Type::Tuple {
            elements: elements.iter().map(|e| resolve_alias(e, aliases)).collect(),
        },
        other => other.clone(),
    }
}

/// Convert string to SCREAMING_SNAKE_CASE
pub fn to_screaming_snake_case(s: &str) -> std::string::String {
    let mut result = std::string::String::new();
    let mut last_was_upper = false;

    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 && !last_was_upper {
            result.push('_');
        }
        result.push(c.to_ascii_uppercase());
        last_was_upper = c.is_uppercase();
    }

    result.replace("__", "_")
}

/// Rust reserved keywords
// r[impl naming.keyword-escape]
pub const RUST_KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "crate", "else", "enum", "extern", "false", "fn", "for",
    "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
    "self", "Self", "static", "struct", "super", "trait", "true", "type", "unsafe", "use",
    "where", "while", "async", "await", "dyn", "abstract", "become", "box", "do", "final",
    "macro", "override", "priv", "typeof", "unsized", "virtual", "yield", "try",
];

#[derive(Debug, thiserror::Error)]
pub enum TypeError {
    #[error("unknown type: {0}")]
    Unknown(std::string::String),
}

#[derive(Debug, thiserror::Error)]
pub enum ValueError {
    #[error("invalid duration format: {0}")]
    InvalidDuration(std::string::String),

    #[error("invalid byte size format: {0}")]
    InvalidBytes(std::string::String),

    #[error("invalid regex: {0}")]
    InvalidRegex(std::string::String),

    #[error("invalid URL: {0}")]
    InvalidUrl(std::string::String),

    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch {
        expected: std::string::String,
        got: std::string::String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_scalar_types() {
        // r[verify type.scalar.i32]
        assert_eq!(parse_type("i32").unwrap(), Type::I32);
        // r[verify type.scalar.string]
        assert_eq!(parse_type("string").unwrap(), Type::String);
    }

    #[test]
    fn test_parse_array_type() {
        // r[verify type.container.array]
        assert_eq!(
            parse_type("string[]").unwrap(),
            Type::Array {
                element: Box::new(Type::String)
            }
        );

        // r[verify type.container.nested-array]
        assert_eq!(
            parse_type("i32[][]").unwrap(),
            Type::Array {
                element: Box::new(Type::Array {
                    element: Box::new(Type::I32)
                })
            }
        );
    }

    #[test]
    fn test_parse_map_type() {
        // r[verify type.container.map]
        assert_eq!(
            parse_type("map<string,i32>").unwrap(),
            Type::Map {
                key: Box::new(Type::String),
                value: Box::new(Type::I32)
            }
        );
    }

    #[test]
    fn test_parse_duration() {
        // r[verify type.duration.format]
        assert_eq!(parse_duration("30s").unwrap(), 30_000_000_000);
        assert_eq!(parse_duration("500ms").unwrap(), 500_000_000);
        assert_eq!(parse_duration("5m").unwrap(), 5 * 60 * 1_000_000_000);
        assert_eq!(parse_duration("2h").unwrap(), 2 * 60 * 60 * 1_000_000_000);
        assert_eq!(parse_duration("1h30m").unwrap(), 90 * 60 * 1_000_000_000);
        assert_eq!(parse_duration("2d").unwrap(), 2 * 24 * 60 * 60 * 1_000_000_000);
    }

    #[test]
    fn test_parse_bytes() {
        // r[verify type.bytes.format]
        assert_eq!(parse_bytes("100").unwrap(), 100);
        assert_eq!(parse_bytes("1KB").unwrap(), 1_000);
        assert_eq!(parse_bytes("1KiB").unwrap(), 1_024);
        assert_eq!(parse_bytes("50MiB").unwrap(), 50 * 1_024 * 1_024);
        assert_eq!(parse_bytes("1GB").unwrap(), 1_000_000_000);
    }

    #[test]
    fn test_is_screaming_snake_case() {
        // r[verify input.constant.naming]
        assert!(is_screaming_snake_case("MAX_RETRIES"));
        assert!(is_screaming_snake_case("TIMEOUT"));
        assert!(is_screaming_snake_case("HTTP_200"));
        assert!(!is_screaming_snake_case("maxRetries"));
        assert!(!is_screaming_snake_case("max_retries"));
        assert!(!is_screaming_snake_case("_LEADING"));
        assert!(!is_screaming_snake_case("TRAILING_"));
        assert!(!is_screaming_snake_case("DOUBLE__UNDERSCORE"));
    }

    #[test]
    fn test_to_camel_case() {
        // r[verify naming.ts]
        assert_eq!(to_camel_case("MAX_RETRIES"), "maxRetries");
        assert_eq!(to_camel_case("TIMEOUT"), "timeout");
        assert_eq!(to_camel_case("HTTP_STATUS_CODE"), "httpStatusCode");
    }

    #[test]
    fn test_to_pascal_case() {
        assert_eq!(to_pascal_case("pending"), "Pending");
        assert_eq!(to_pascal_case("active"), "Active");
        assert_eq!(to_pascal_case("some_value"), "SomeValue");
    }

    #[test]
    fn test_parse_struct_type() {
        // r[verify type.struct.infer]
        // Struct type is parsed as empty and filled in by parser
        assert!(matches!(parse_type("struct").unwrap(), Type::Struct { fields } if fields.is_empty()));
    }

    #[test]
    fn test_parse_tuple_type() {
        // r[verify type.container.tuple]
        let typ = parse_type("tuple<i32, string, bool>").unwrap();
        match typ {
            Type::Tuple { elements } => {
                assert_eq!(elements.len(), 3);
                assert_eq!(elements[0], Type::I32);
                assert_eq!(elements[1], Type::String);
                assert_eq!(elements[2], Type::Bool);
            }
            _ => panic!("expected tuple type"),
        }
    }
}
