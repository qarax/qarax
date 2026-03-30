pub fn normalize_architecture(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let normalized = match lower.as_str() {
        "amd64" => "x86_64",
        "arm64" => "aarch64",
        other => other,
    };
    Some(normalized.to_string())
}

pub fn current_architecture() -> String {
    normalize_architecture(std::env::consts::ARCH).unwrap_or_else(|| std::env::consts::ARCH.into())
}
