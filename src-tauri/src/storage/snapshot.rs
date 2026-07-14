//! Snapshot / delta producer foundations.
//!
//! Independent of the task actor: reads from a SQLite snapshot under a fixed barrier
//! `(lastSequence=B, generation=G, uiStateRevision=U)`. Live events go through a
//! per-subscriber bounded backlog and are only forwarded after `snapshot_end`.

use crate::ipc::codec::{SNAPSHOT_CHUNK_MAX_BYTES, SNAPSHOT_FRAGMENT_RAW_MAX};
use crate::storage::repository::{
    list_mutations_after, list_timeline_items, list_ui_state, ui_state_generation,
    ui_state_revision, MutationRow, TimelineItemRow, UiStateRow,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use thiserror::Error;

/// Default backlog: 10_000 events or 16 MiB (whichever first).
pub const BACKLOG_MAX_EVENTS: usize = 10_000;
pub const BACKLOG_MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("repo: {0}")]
    Repo(#[from] crate::storage::repository::RepoError),
    #[error("subscriber backlog exceeded")]
    BacklogExceeded,
    #[error("fragment hash mismatch")]
    FragmentHashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeParams {
    pub task_id: String,
    pub surface_id: String,
    pub selection_epoch: u64,
    pub subscription_epoch: u64,
    pub stream_id: String,
    #[serde(default)]
    pub after_sequence: Option<i64>,
    #[serde(default)]
    pub generation: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotHeader {
    pub mode: String,
    pub task_id: String,
    pub surface_id: String,
    pub selection_epoch: u64,
    pub subscription_epoch: u64,
    pub stream_id: String,
    pub generation: i64,
    pub last_sequence: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_sequence: Option<i64>,
    pub ui_state_generation: String,
    pub ui_state_revision: i64,
    pub timeline_entry_count: usize,
    pub ui_state_row_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_plan_item_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamFrame {
    Header(SnapshotHeader),
    Chunk {
        chunk_kind: &'static str,
        chunk_index: u32,
        entries: Vec<Value>,
    },
    ItemFragment {
        entry_id: String,
        fragment_index: u32,
        fragment_count: u32,
        total_bytes: usize,
        sha256: String,
        data_b64: String,
    },
    End {
        generation: i64,
        last_sequence: i64,
        ui_state_generation: String,
        ui_state_revision: i64,
        item_count: usize,
        fragment_count: u32,
    },
    Live(Value),
}

impl StreamFrame {
    pub fn to_event_json(&self, params: &SubscribeParams) -> Value {
        match self {
            StreamFrame::Header(h) => json!({
                "type": "response",
                "ok": true,
                "result": h,
            }),
            StreamFrame::Chunk {
                chunk_kind,
                chunk_index,
                entries,
            } => json!({
                "type": "event",
                "event": "task.snapshot_chunk",
                "taskId": params.task_id,
                "surfaceId": params.surface_id,
                "selectionEpoch": params.selection_epoch,
                "subscriptionEpoch": params.subscription_epoch,
                "streamId": params.stream_id,
                "chunkKind": chunk_kind,
                "chunkIndex": chunk_index,
                "entries": entries,
            }),
            StreamFrame::ItemFragment {
                entry_id,
                fragment_index,
                fragment_count,
                total_bytes,
                sha256,
                data_b64,
            } => json!({
                "type": "event",
                "event": "task.snapshot_item_fragment",
                "taskId": params.task_id,
                "surfaceId": params.surface_id,
                "selectionEpoch": params.selection_epoch,
                "subscriptionEpoch": params.subscription_epoch,
                "streamId": params.stream_id,
                "entryId": entry_id,
                "fragmentIndex": fragment_index,
                "fragmentCount": fragment_count,
                "totalBytes": total_bytes,
                "sha256": sha256,
                "data": data_b64,
            }),
            StreamFrame::End {
                generation,
                last_sequence,
                ui_state_generation,
                ui_state_revision,
                item_count,
                fragment_count,
            } => json!({
                "type": "event",
                "event": "task.snapshot_end",
                "taskId": params.task_id,
                "surfaceId": params.surface_id,
                "selectionEpoch": params.selection_epoch,
                "subscriptionEpoch": params.subscription_epoch,
                "streamId": params.stream_id,
                "generation": generation,
                "lastSequence": last_sequence,
                "uiStateGeneration": ui_state_generation,
                "uiStateRevision": ui_state_revision,
                "itemCount": item_count,
                "fragmentCount": fragment_count,
            }),
            StreamFrame::Live(v) => v.clone(),
        }
    }
}

/// Barrier capture point for a subscription.
#[derive(Debug, Clone)]
pub struct SnapshotBarrier {
    pub last_sequence: i64,
    pub generation: i64,
    pub ui_generation: String,
    pub ui_revision: i64,
}

/// Capture barrier from an independent read connection (call inside storage cutover).
pub fn capture_barrier(conn: &Connection, task_id: &str) -> Result<SnapshotBarrier, SnapshotError> {
    let (last_sequence, generation): (i64, i64) = conn
        .query_row(
            "SELECT last_sequence, timeline_generation FROM tasks WHERE id = ?1",
            rusqlite::params![task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| SnapshotError::Repo(e.into()))?;
    Ok(SnapshotBarrier {
        last_sequence,
        generation,
        ui_generation: ui_state_generation(conn)?,
        ui_revision: ui_state_revision(conn)?,
    })
}

/// Decide full snapshot vs delta for current barrier.
pub fn choose_mode(
    conn: &Connection,
    task_id: &str,
    barrier: &SnapshotBarrier,
    after_sequence: Option<i64>,
    client_generation: Option<i64>,
) -> Result<(&'static str, Option<Vec<MutationRow>>), SnapshotError> {
    if let (Some(after), Some(gen)) = (after_sequence, client_generation) {
        if gen == barrier.generation && after < barrier.last_sequence {
            let muts = list_mutations_after(conn, task_id, gen, after)?;
            // Ensure contiguous coverage after+1..B
            if !muts.is_empty()
                && muts.first().map(|m| m.sequence) == Some(after + 1)
                && muts.last().map(|m| m.sequence) == Some(barrier.last_sequence)
            {
                return Ok(("delta", Some(muts)));
            }
        }
    }
    Ok(("full", None))
}

/// Build the ordered frame list for the snapshot phase (no live frames).
pub fn produce_snapshot_frames(
    conn: &Connection,
    params: &SubscribeParams,
    barrier: &SnapshotBarrier,
) -> Result<Vec<StreamFrame>, SnapshotError> {
    let (mode, delta_muts) = choose_mode(
        conn,
        &params.task_id,
        barrier,
        params.after_sequence,
        params.generation,
    )?;

    let ui_rows = list_ui_state(conn, &params.task_id)?;
    let mut frames = Vec::new();
    let mut fragment_count = 0u32;

    if mode == "delta" {
        let muts = delta_muts.unwrap_or_default();
        frames.push(StreamFrame::Header(SnapshotHeader {
            mode: "delta".into(),
            task_id: params.task_id.clone(),
            surface_id: params.surface_id.clone(),
            selection_epoch: params.selection_epoch,
            subscription_epoch: params.subscription_epoch,
            stream_id: params.stream_id.clone(),
            generation: barrier.generation,
            last_sequence: barrier.last_sequence,
            from_sequence: params.after_sequence,
            ui_state_generation: barrier.ui_generation.clone(),
            ui_state_revision: barrier.ui_revision,
            timeline_entry_count: muts.len(),
            ui_state_row_count: ui_rows.len(),
            active_plan_item_id: None,
        }));
        let (chunk_frames, frags) =
            chunk_entries("timeline_mutations", muts.iter().map(mutation_to_value))?;
        fragment_count += frags;
        frames.extend(chunk_frames);
    } else {
        let items = list_timeline_items(conn, &params.task_id)?;
        let active_plan = items
            .iter()
            .find(|i| i.kind == "plan")
            .map(|i| i.item_id.clone());
        frames.push(StreamFrame::Header(SnapshotHeader {
            mode: "full".into(),
            task_id: params.task_id.clone(),
            surface_id: params.surface_id.clone(),
            selection_epoch: params.selection_epoch,
            subscription_epoch: params.subscription_epoch,
            stream_id: params.stream_id.clone(),
            generation: barrier.generation,
            last_sequence: barrier.last_sequence,
            from_sequence: None,
            ui_state_generation: barrier.ui_generation.clone(),
            ui_state_revision: barrier.ui_revision,
            timeline_entry_count: items.len(),
            ui_state_row_count: ui_rows.len(),
            active_plan_item_id: active_plan,
        }));
        let (chunk_frames, frags) =
            chunk_entries("timeline_items", items.iter().map(item_to_value))?;
        fragment_count += frags;
        frames.extend(chunk_frames);
    }

    let (ui_frames, ui_frags) = chunk_entries("ui_state_rows", ui_rows.iter().map(ui_to_value))?;
    fragment_count += ui_frags;
    frames.extend(ui_frames);

    let item_count = match frames.first() {
        Some(StreamFrame::Header(h)) => h.timeline_entry_count + h.ui_state_row_count,
        _ => 0,
    };

    frames.push(StreamFrame::End {
        generation: barrier.generation,
        last_sequence: barrier.last_sequence,
        ui_state_generation: barrier.ui_generation.clone(),
        ui_state_revision: barrier.ui_revision,
        item_count,
        fragment_count,
    });

    Ok(frames)
}

fn item_to_value(i: &TimelineItemRow) -> Value {
    json!({
        "taskId": i.task_id,
        "itemId": i.item_id,
        "turnId": i.turn_id,
        "kind": i.kind,
        "firstSequence": i.first_sequence,
        "lastSequence": i.last_sequence,
        "payload": serde_json::from_str::<Value>(&i.payload_json).unwrap_or(Value::Null),
    })
}

fn mutation_to_value(m: &MutationRow) -> Value {
    json!({
        "taskId": m.task_id,
        "sequence": m.sequence,
        "generation": m.generation,
        "operation": m.operation,
        "itemId": m.item_id,
        "payload": serde_json::from_str::<Value>(&m.payload_json).unwrap_or(Value::Null),
    })
}

fn ui_to_value(u: &UiStateRow) -> Value {
    json!({
        "taskId": u.task_id,
        "disclosureKey": u.disclosure_key,
        "expansion": u.expansion,
        "revision": u.revision,
    })
}

fn chunk_entries<I>(
    kind: &'static str,
    entries: I,
) -> Result<(Vec<StreamFrame>, u32), SnapshotError>
where
    I: IntoIterator<Item = Value>,
{
    let mut frames = Vec::new();
    let mut chunk_index = 0u32;
    let mut current: Vec<Value> = Vec::new();
    let mut current_bytes = 2usize; // []
    let mut fragment_count = 0u32;

    for entry in entries {
        let encoded = serde_json::to_vec(&entry).unwrap_or_default();
        if encoded.len() > SNAPSHOT_CHUNK_MAX_BYTES {
            // Flush current chunk first.
            if !current.is_empty() {
                frames.push(StreamFrame::Chunk {
                    chunk_kind: kind,
                    chunk_index,
                    entries: std::mem::take(&mut current),
                });
                chunk_index += 1;
                current_bytes = 2;
            }
            let (frags, n) = fragment_entry(&entry, &encoded)?;
            fragment_count += n;
            frames.extend(frags);
            continue;
        }
        let add = encoded.len() + 1;
        if !current.is_empty() && current_bytes + add > SNAPSHOT_CHUNK_MAX_BYTES {
            frames.push(StreamFrame::Chunk {
                chunk_kind: kind,
                chunk_index,
                entries: std::mem::take(&mut current),
            });
            chunk_index += 1;
            current_bytes = 2;
        }
        current_bytes += add;
        current.push(entry);
    }
    if !current.is_empty() {
        frames.push(StreamFrame::Chunk {
            chunk_kind: kind,
            chunk_index,
            entries: current,
        });
    }
    Ok((frames, fragment_count))
}

fn fragment_entry(entry: &Value, bytes: &[u8]) -> Result<(Vec<StreamFrame>, u32), SnapshotError> {
    let entry_id = entry
        .get("itemId")
        .or_else(|| entry.get("disclosureKey"))
        .and_then(|v| v.as_str())
        .unwrap_or("entry")
        .to_string();
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha = hex::encode(hasher.finalize());
    let total = bytes.len();
    let mut frames = Vec::new();
    let chunks: Vec<&[u8]> = bytes.chunks(SNAPSHOT_FRAGMENT_RAW_MAX).collect();
    let count = chunks.len() as u32;
    for (index, chunk) in chunks.into_iter().enumerate() {
        frames.push(StreamFrame::ItemFragment {
            entry_id: entry_id.clone(),
            fragment_index: index as u32,
            fragment_count: count,
            total_bytes: total,
            sha256: sha.clone(),
            data_b64: B64.encode(chunk),
        });
    }
    Ok((frames, count))
}

/// Reassemble fragments and verify sha256 (client-side helper, tested here).
pub fn reassemble_fragments(
    fragments: &[(u32, Vec<u8>)],
    total_bytes: usize,
    expected_sha: &str,
) -> Result<Vec<u8>, SnapshotError> {
    let mut ordered = fragments.to_vec();
    ordered.sort_by_key(|(i, _)| *i);
    let mut out = Vec::with_capacity(total_bytes);
    for (_, part) in ordered {
        out.extend(part);
    }
    if out.len() != total_bytes {
        return Err(SnapshotError::FragmentHashMismatch);
    }
    let mut hasher = Sha256::new();
    hasher.update(&out);
    let sha = hex::encode(hasher.finalize());
    if sha != expected_sha {
        return Err(SnapshotError::FragmentHashMismatch);
    }
    Ok(out)
}

/// Per-subscriber ordered backlog for live events during/after snapshot.
#[derive(Debug)]
pub struct SubscriberBacklog {
    queue: VecDeque<Value>,
    bytes: usize,
    max_events: usize,
    max_bytes: usize,
    /// Live events are held until snapshot_end is emitted.
    snapshot_done: bool,
    dropped: bool,
}

impl SubscriberBacklog {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            bytes: 0,
            max_events: BACKLOG_MAX_EVENTS,
            max_bytes: BACKLOG_MAX_BYTES,
            snapshot_done: false,
            dropped: false,
        }
    }

    pub fn with_limits(max_events: usize, max_bytes: usize) -> Self {
        Self {
            max_events,
            max_bytes,
            ..Self::new()
        }
    }

    /// Non-blocking push of a live event (>B). Returns Err if backlog exceeded.
    pub fn push_live(&mut self, event: Value) -> Result<(), SnapshotError> {
        if self.dropped {
            return Err(SnapshotError::BacklogExceeded);
        }
        let size = serde_json::to_vec(&event).map(|v| v.len()).unwrap_or(0);
        if self.queue.len() + 1 > self.max_events || self.bytes + size > self.max_bytes {
            self.dropped = true;
            self.queue.clear();
            self.bytes = 0;
            return Err(SnapshotError::BacklogExceeded);
        }
        self.bytes += size;
        self.queue.push_back(event);
        Ok(())
    }

    pub fn mark_snapshot_done(&mut self) {
        self.snapshot_done = true;
    }

    pub fn snapshot_done(&self) -> bool {
        self.snapshot_done
    }

    /// Drain live events only after snapshot_end.
    pub fn drain_live(&mut self) -> Vec<Value> {
        if !self.snapshot_done {
            return Vec::new();
        }
        self.bytes = 0;
        self.queue.drain(..).collect()
    }

    pub fn pending_live_count(&self) -> usize {
        self.queue.len()
    }

    pub fn is_dropped(&self) -> bool {
        self.dropped
    }
}

impl Default for SubscriberBacklog {
    fn default() -> Self {
        Self::new()
    }
}

/// Full subscribe pipeline: snapshot frames then backlog live (for tests / producer worker).
pub fn run_subscribe_pipeline(
    conn: &Connection,
    params: &SubscribeParams,
    backlog: &mut SubscriberBacklog,
) -> Result<Vec<StreamFrame>, SnapshotError> {
    let barrier = capture_barrier(conn, &params.task_id)?;
    let mut frames = produce_snapshot_frames(conn, params, &barrier)?;
    backlog.mark_snapshot_done();
    for live in backlog.drain_live() {
        frames.push(StreamFrame::Live(live));
    }
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::open_memory;
    use crate::storage::repository::{
        commit_timeline_mutations, insert_task, MutationRow, TaskRow, TimelineItemRow,
    };

    fn task(id: &str) -> TaskRow {
        let t = 1;
        TaskRow {
            id: id.into(),
            title: "t".into(),
            cwd: "/tmp".into(),
            mode: "read".into(),
            status: "idle".into(),
            session_state: Some("cold".into()),
            recovery_state: Some("none".into()),
            active_recovery_id: None,
            last_turn_id: None,
            acp_session_id: None,
            daemon_instance_id: None,
            supervisor_pid: None,
            supervisor_started_at: None,
            retention_protect_until: None,
            last_sequence: 0,
            timeline_generation: 1,
            state_revision: 1,
            created_at: t,
            updated_at: t,
            finished_at: None,
        }
    }

    fn params(task_id: &str) -> SubscribeParams {
        SubscribeParams {
            task_id: task_id.into(),
            surface_id: "popover-main".into(),
            selection_epoch: 1,
            subscription_epoch: 1,
            stream_id: "stream-1".into(),
            after_sequence: None,
            generation: None,
        }
    }

    #[test]
    fn barrier_snapshot_end_before_live() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &task("t1")).unwrap();
        let item = TimelineItemRow {
            task_id: "t1".into(),
            item_id: "i1".into(),
            turn_id: None,
            kind: "assistant_segment".into(),
            first_sequence: 1,
            last_sequence: 1,
            payload_json: r#"{"text":"a"}"#.into(),
            created_at: 1,
            updated_at: 1,
        };
        let m = MutationRow {
            task_id: "t1".into(),
            sequence: 1,
            generation: 1,
            operation: "add".into(),
            item_id: Some("i1".into()),
            payload_json: r#"{"text":"a"}"#.into(),
            created_at: 1,
        };
        commit_timeline_mutations(&conn, "t1", &[item], &[m], 1).unwrap();

        let mut backlog = SubscriberBacklog::new();
        // Live event arrives during snapshot production (simultaneous mutation).
        backlog
            .push_live(json!({"event":"task.mutation","sequence":2}))
            .unwrap();
        assert_eq!(backlog.pending_live_count(), 1);
        // Before snapshot_end, drain returns empty.
        assert!(backlog.drain_live().is_empty());

        let frames = run_subscribe_pipeline(&conn, &params("t1"), &mut backlog).unwrap();
        let end_pos = frames
            .iter()
            .position(|f| matches!(f, StreamFrame::End { .. }))
            .unwrap();
        let live_pos = frames
            .iter()
            .position(|f| matches!(f, StreamFrame::Live(_)))
            .unwrap();
        assert!(end_pos < live_pos, "snapshot_end must precede live");
        // Header first
        assert!(matches!(frames[0], StreamFrame::Header(_)));
    }

    #[test]
    fn backlog_disconnects_slow_subscriber() {
        let mut backlog = SubscriberBacklog::with_limits(2, 1024);
        backlog.push_live(json!({"a":1})).unwrap();
        backlog.push_live(json!({"a":2})).unwrap();
        let err = backlog.push_live(json!({"a":3})).unwrap_err();
        assert!(matches!(err, SnapshotError::BacklogExceeded));
        assert!(backlog.is_dropped());
    }

    #[test]
    fn large_item_fragments_and_reassemble() {
        // Build entry larger than 1 MiB.
        let big_text = "x".repeat(SNAPSHOT_CHUNK_MAX_BYTES + 100);
        let entry = json!({"itemId": "big", "payload": {"text": big_text}});
        let bytes = serde_json::to_vec(&entry).unwrap();
        assert!(bytes.len() > SNAPSHOT_CHUNK_MAX_BYTES);
        let (frames, n) = fragment_entry(&entry, &bytes).unwrap();
        assert!(n >= 2);
        assert_eq!(frames.len() as u32, n);
        let mut parts = Vec::new();
        let mut sha = String::new();
        let mut total = 0usize;
        for f in &frames {
            if let StreamFrame::ItemFragment {
                fragment_index,
                total_bytes,
                sha256,
                data_b64,
                ..
            } = f
            {
                total = *total_bytes;
                sha = sha256.clone();
                parts.push((*fragment_index, B64.decode(data_b64).unwrap()));
            }
        }
        let out = reassemble_fragments(&parts, total, &sha).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn ui_state_included_in_same_barrier() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &task("t1")).unwrap();
        crate::storage::repository::ui_state_set(&conn, "t1", "item:1:details", "user-expanded")
            .unwrap();
        let barrier = capture_barrier(&conn, "t1").unwrap();
        let frames = produce_snapshot_frames(&conn, &params("t1"), &barrier).unwrap();
        let has_ui = frames.iter().any(|f| {
            matches!(
                f,
                StreamFrame::Chunk {
                    chunk_kind: "ui_state_rows",
                    ..
                }
            )
        });
        assert!(has_ui);
        if let StreamFrame::Header(h) = &frames[0] {
            assert_eq!(h.ui_state_revision, barrier.ui_revision);
            assert_eq!(h.ui_state_generation, barrier.ui_generation);
        } else {
            panic!("expected header");
        }
    }

    #[test]
    fn generation_reset_forces_full_resnapshot() {
        let conn = open_memory().unwrap();
        insert_task(&conn, &task("t1")).unwrap();
        let m = MutationRow {
            task_id: "t1".into(),
            sequence: 1,
            generation: 1,
            operation: "add".into(),
            item_id: Some("i1".into()),
            payload_json: "{}".into(),
            created_at: 1,
        };
        let item = TimelineItemRow {
            task_id: "t1".into(),
            item_id: "i1".into(),
            turn_id: None,
            kind: "assistant_segment".into(),
            first_sequence: 1,
            last_sequence: 1,
            payload_json: "{}".into(),
            created_at: 1,
            updated_at: 1,
        };
        commit_timeline_mutations(&conn, "t1", &[item], &[m], 1).unwrap();
        let (new_gen, _) =
            crate::storage::repository::timeline_generation_reset(&conn, "t1").unwrap();
        assert_eq!(new_gen, 2);
        let barrier = capture_barrier(&conn, "t1").unwrap();
        // Client still on gen 1 + after_sequence — must get full, not delta.
        let mut p = params("t1");
        p.after_sequence = Some(0);
        p.generation = Some(1);
        let (mode, _) = choose_mode(&conn, "t1", &barrier, p.after_sequence, p.generation).unwrap();
        assert_eq!(mode, "full");
    }
}
