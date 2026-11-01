//! JSON shapes of the hub API, shared by hub and client.

use serde::{Deserialize, Serialize};

use crate::envelope::{Envelope, SeqEnvelope};

/// `GET /v1/hello` (no auth, no secrets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub proto: u32,
    pub space_id: String,
    pub hub_device_id: String,
    pub hub_name: String,
    pub head_seq: i64,
    /// `host:port` addresses the hub currently advertises.
    pub hosts: Vec<String>,
}

/// `GET /v1/events?since=N&limit=M`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventsPage {
    pub events: Vec<SeqEnvelope>,
    pub head_seq: i64,
}

/// `POST /v1/events` body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushRequest {
    pub events: Vec<Envelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rejected {
    pub event_id: String,
    pub reason: String,
}

/// `POST /v1/events` reply. Accepted includes duplicates (idempotent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    pub accepted: Vec<String>,
    pub rejected: Vec<Rejected>,
    pub head_seq: i64,
}

/// One text frame on `/v1/stream`: `{"t":"event","seq":N,...Envelope}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamFrame {
    pub t: String,
    #[serde(flatten)]
    pub ev: SeqEnvelope,
}
