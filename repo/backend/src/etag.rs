//! SHA-256 ETag helper for offline-first sync (PRD §8).

use sha2::{Digest, Sha256};

pub fn compute(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    hex::encode(h.finalize())
}

pub fn from_parts<I: IntoIterator<Item = S>, S: AsRef<[u8]>>(parts: I) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_ref());
        h.update(b"|");
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_across_calls() {
        assert_eq!(compute(b"abc"), compute(b"abc"));
    }

    #[test]
    fn differs_when_input_differs() {
        assert_ne!(compute(b"abc"), compute(b"abd"));
    }

    #[test]
    fn from_parts_delimited() {
        let a = from_parts(["a", "bc"]);
        let b = from_parts(["ab", "c"]);
        assert_ne!(a, b);
    }
}
