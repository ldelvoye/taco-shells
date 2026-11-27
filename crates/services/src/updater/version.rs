fn version_parts(text: &str) -> Option<[u64; 3]> {
    let pieces: Vec<&str> = text.split('.').collect();
    if pieces.len() != 3 {
        return None;
    }
    let mut parts = [0u64; 3];
    for (slot, piece) in parts.iter_mut().zip(pieces) {
        if piece.is_empty() || !piece.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let parsed = piece.parse::<u64>();
        let Ok(number) = parsed else {
            return None;
        };
        *slot = number;
    }
    Some(parts)
}

pub(crate) fn is_version(text: &str) -> bool {
    version_parts(text).is_some()
}

pub(crate) fn is_newer_version(candidate: &str, current: &str) -> bool {
    let Some(candidate_parts) = version_parts(candidate) else {
        return false;
    };
    let Some(current_parts) = version_parts(current) else {
        return false;
    };
    candidate_parts > current_parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically_and_only_when_plain() {
        assert!(is_newer_version("0.10.0", "0.9.9"));
        assert!(!is_newer_version("0.9.9", "0.10.0"));
        assert!(!is_newer_version("1.2.3", "1.2.3"));
        assert!(!is_newer_version("1.2.4-beta", "1.2.3"));
        assert!(!is_newer_version("1.2.3", "1.2.4-beta"));
        assert!(!is_newer_version("1.2", "1.1.0"));
    }
}
