//! Tamper-evident, append-only audit chain (NFR-08, P7, EMG-05).
//! Each entry hashes the previous entry's hash together with its own canonical content.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditEntry {
    pub seq: u64,
    pub at: DateTime<Utc>,
    pub actor: String,
    pub action: String,
    pub subject: String,
    pub detail: serde_json::Value,
    pub prev_hash: String,
    pub hash: String,
}

fn digest(
    seq: u64,
    at: &DateTime<Utc>,
    actor: &str,
    action: &str,
    subject: &str,
    detail: &serde_json::Value,
    prev: &str,
) -> String {
    let mut h = Sha256::new();
    h.update(seq.to_be_bytes());
    h.update(
        at.to_rfc3339_opts(chrono::SecondsFormat::Micros, true)
            .as_bytes(),
    );
    for part in [actor, action, subject] {
        h.update((part.len() as u64).to_be_bytes());
        h.update(part.as_bytes());
    }
    h.update(serde_json::to_vec(detail).unwrap_or_default());
    h.update(prev.as_bytes());
    hex::encode(h.finalize())
}

/// Hex SHA-256 of a byte slice (used for artefact integrity checks).
pub fn sha256_hex(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AuditChain {
    pub entries: Vec<AuditEntry>,
}

impl AuditChain {
    pub fn append(
        &mut self,
        actor: &str,
        action: &str,
        subject: &str,
        detail: serde_json::Value,
    ) -> AuditEntry {
        let seq = self.entries.len() as u64 + 1;
        let prev = self
            .entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| GENESIS.to_string());
        // Microsecond precision so the stored timestamp round-trips exactly.
        let at =
            DateTime::from_timestamp_micros(Utc::now().timestamp_micros()).unwrap_or_else(Utc::now);
        let hash = digest(seq, &at, actor, action, subject, &detail, &prev);
        let e = AuditEntry {
            seq,
            at,
            actor: actor.into(),
            action: action.into(),
            subject: subject.into(),
            detail,
            prev_hash: prev,
            hash,
        };
        self.entries.push(e.clone());
        e
    }

    /// Returns the sequence number of the first broken link, or None if the chain is intact.
    pub fn verify(&self) -> Option<u64> {
        verify_entries(&self.entries)
    }
}

pub fn verify_entries(entries: &[AuditEntry]) -> Option<u64> {
    let mut prev = GENESIS.to_string();
    for (i, e) in entries.iter().enumerate() {
        let expect = digest(
            e.seq, &e.at, &e.actor, &e.action, &e.subject, &e.detail, &prev,
        );
        // Sequence numbers are contiguous from 1, so a renumbered chain is detected too.
        if e.seq != i as u64 + 1 || e.prev_hash != prev || e.hash != expect {
            return Some(e.seq);
        }
        prev = e.hash.clone();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn detects_tampering() {
        let mut c = AuditChain::default();
        c.append(
            "eng.a",
            "grade",
            "H1",
            serde_json::json!({"to": "corroborated"}),
        );
        c.append("eng.b", "close_case", "C1", serde_json::json!({}));
        assert_eq!(c.verify(), None);
        c.entries[0].actor = "intruder".into();
        assert_eq!(c.verify(), Some(1));
    }
}
