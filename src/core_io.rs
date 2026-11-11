//! Bounded JSONL framing for pipes and authenticated local sockets.
use std::io::{self, BufRead};
pub const MAX_LINE: usize = 2 * 1024 * 1024 + 4096;
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Data,
    TooLong,
    Eof,
}
pub struct Lines<R> {
    reader: R,
    discarding: bool,
    /// Partial line kept across read timeouts so a slow writer is not cut off mid-message.
    pending: Vec<u8>,
}
impl<R: BufRead> Lines<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            discarding: false,
            pending: Vec::new(),
        }
    }
    /// Reads the next line into `out`. A `TimedOut`/`WouldBlock`/`Interrupted` error is
    /// resumable: call again and the partially received line is continued, not dropped.
    pub fn next(&mut self, out: &mut Vec<u8>, limit: usize) -> io::Result<Line> {
        out.clear();
        loop {
            let bytes = match self.reader.fill_buf() {
                Ok(bytes) => bytes,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if bytes.is_empty() {
                if self.pending.is_empty() {
                    return Ok(Line::Eof);
                }
                std::mem::swap(out, &mut self.pending);
                return Ok(Line::Data);
            }
            let end = bytes.iter().position(|b| *b == b'\n');
            let count = end.map_or(bytes.len(), |i| i + 1);
            if self.discarding {
                self.reader.consume(count);
                if end.is_some() {
                    self.discarding = false;
                }
                continue;
            }
            if self.pending.len().saturating_add(count) > limit {
                self.reader.consume(count);
                self.discarding = end.is_none();
                self.pending.clear();
                return Ok(Line::TooLong);
            }
            self.pending.extend_from_slice(&bytes[..count]);
            self.reader.consume(count);
            if end.is_some() {
                self.pending.pop();
                if self.pending.last() == Some(&b'\r') {
                    self.pending.pop();
                }
                std::mem::swap(out, &mut self.pending);
                return Ok(Line::Data);
            }
        }
    }
}
/// Commands whose newest value fully replaces older queued ones (sliders, colour pickers).
/// Commands carrying a `requestId` are never coalesced because a caller waits on each.
pub fn coalesce_key(value: &serde_json::Value) -> Option<String> {
    const LATEST_WINS: &[&str] = &[
        "applyTriggers",
        "setLightbar",
        "setPlayerLeds",
        "setAudioMixer",
        "setControlProfile",
        "setMotionBias",
        "setGyroAim",
        "setPersonalHaptics",
    ];
    let action = value.get("action")?.as_str()?;
    (LATEST_WINS.contains(&action) && value.get("requestId").is_none()).then(|| action.to_owned())
}

pub type Item = (Option<String>, Vec<u8>);
#[derive(Debug, PartialEq, Eq)]
pub struct Full;
/// Bounded FIFO of encoded command lines with latest-wins coalescing. Pushing never blocks.
pub struct Outbox {
    cap: usize,
    queue: std::sync::Mutex<std::collections::VecDeque<Item>>,
    ready: std::sync::Condvar,
    /// Bumped by every `clear`, so a consumer can tell that an item it popped earlier was
    /// discarded-by-intent in the meantime.
    epoch: std::sync::atomic::AtomicU64,
}
impl Outbox {
    pub fn new(cap: usize) -> Self {
        Self {
            cap,
            queue: Default::default(),
            ready: Default::default(),
            epoch: Default::default(),
        }
    }
    pub fn epoch(&self) -> u64 {
        self.epoch.load(std::sync::atomic::Ordering::SeqCst)
    }
    fn lock(&self) -> std::sync::MutexGuard<'_, std::collections::VecDeque<Item>> {
        self.queue.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn push(&self, key: Option<String>, line: Vec<u8>) -> Result<(), Full> {
        let mut queue = self.lock();
        if let Some(key) = &key
            && let Some(index) = queue.iter().position(|(k, _)| k.as_ref() == Some(key))
        {
            // Replace by removing the stale entry and queueing the new one at the back, so
            // the newest value never jumps ahead of commands that were sent after the old one.
            queue.remove(index);
        } else if queue.len() >= self.cap {
            return Err(Full);
        }
        queue.push_back((key, line));
        drop(queue);
        self.ready.notify_one();
        Ok(())
    }
    /// Ignores the cap; for the few control lines that must not be lost.
    pub fn push_force(&self, line: Vec<u8>) {
        self.lock().push_back((None, line));
        self.ready.notify_one();
    }
    pub fn pop_wait(&self, timeout: std::time::Duration) -> Option<Item> {
        self.pop_wait_epoch(timeout).map(|(item, _)| item)
    }
    /// Like `pop_wait`, with the epoch the item was popped in. If `epoch()` differs later, the
    /// queue was cleared after the pop and the item belongs to a discarded generation.
    pub fn pop_wait_epoch(&self, timeout: std::time::Duration) -> Option<(Item, u64)> {
        let mut queue = self.lock();
        if queue.is_empty() {
            queue = self
                .ready
                .wait_timeout(queue, timeout)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        let epoch = self.epoch();
        queue.pop_front().map(|item| (item, epoch))
    }
    pub fn clear(&self) {
        let mut queue = self.lock();
        queue.clear();
        self.epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.lock().len()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oversized_frames_are_bounded_and_the_next_command_survives() {
        let mut bytes = vec![b'x'; 100_000];
        bytes.extend_from_slice(b"\n{\"action\":\"scan\"}\r\n");
        let mut reader = Lines::new(std::io::BufReader::with_capacity(
            23,
            std::io::Cursor::new(bytes),
        ));
        let mut out = Vec::new();
        assert_eq!(reader.next(&mut out, 1024).unwrap(), Line::TooLong);
        assert!(out.len() <= 1024);
        assert_eq!(reader.next(&mut out, 1024).unwrap(), Line::Data);
        assert_eq!(out, b"{\"action\":\"scan\"}");
        assert_eq!(reader.next(&mut out, 1024).unwrap(), Line::Eof);
    }
    #[test]
    fn fragmented_lines_and_final_unterminated_line_are_preserved() {
        let mut reader = Lines::new(std::io::BufReader::with_capacity(
            1,
            std::io::Cursor::new(b"abc\nlast"),
        ));
        let mut out = Vec::new();
        assert_eq!(reader.next(&mut out, 8).unwrap(), Line::Data);
        assert_eq!(out, b"abc");
        assert_eq!(reader.next(&mut out, 8).unwrap(), Line::Data);
        assert_eq!(out, b"last");
        assert_eq!(reader.next(&mut out, 8).unwrap(), Line::Eof);
    }
    #[test]
    fn coalescing_keeps_order_relative_to_other_commands() {
        let outbox = Outbox::new(8);
        let key = Some("setLightbar".to_string());
        outbox
            .push(
                key.clone(),
                b"A
"
                .to_vec(),
            )
            .unwrap();
        outbox
            .push(
                None,
                b"release
"
                .to_vec(),
            )
            .unwrap();
        outbox
            .push(
                key,
                b"B
"
                .to_vec(),
            )
            .unwrap();
        let t = std::time::Duration::from_millis(1);
        assert_eq!(
            outbox.pop_wait(t).unwrap().1,
            b"release
"
        );
        assert_eq!(
            outbox.pop_wait(t).unwrap().1,
            b"B
"
        );
        assert!(outbox.pop_wait(t).is_none());
    }
    #[test]
    fn partial_line_survives_a_read_timeout() {
        struct Flaky(Vec<Result<&'static [u8], io::ErrorKind>>);
        impl io::Read for Flaky {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                match self.0.remove(0) {
                    Ok(b) => {
                        buf[..b.len()].copy_from_slice(b);
                        Ok(b.len())
                    }
                    Err(k) => Err(k.into()),
                }
            }
        }
        let mut reader = Lines::new(io::BufReader::new(Flaky(vec![
            Ok(b"{\"action\":"),
            Err(io::ErrorKind::TimedOut),
            Ok(b"\"scan\"}
"),
        ])));
        let mut out = Vec::new();
        assert_eq!(
            reader.next(&mut out, 1024).unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert_eq!(reader.next(&mut out, 1024).unwrap(), Line::Data);
        assert_eq!(out, b"{\"action\":\"scan\"}");
    }
    #[test]
    fn clear_bumps_the_epoch_of_items_popped_before_it() {
        let outbox = Outbox::new(4);
        outbox.push(None, b"a".to_vec()).unwrap();
        let t = std::time::Duration::from_millis(1);
        let (item, epoch) = outbox.pop_wait_epoch(t).unwrap();
        assert_eq!(item.1, b"a");
        assert_eq!(epoch, outbox.epoch());
        outbox.clear();
        assert_ne!(epoch, outbox.epoch());
        outbox.push(None, b"b".to_vec()).unwrap();
        let (_, fresh) = outbox.pop_wait_epoch(t).unwrap();
        assert_eq!(fresh, outbox.epoch());
    }
    #[test]
    fn outbox_coalesces_sliders_and_rejects_overflow() {
        let outbox = Outbox::new(2);
        let slider = serde_json::json!({"action":"setLightbar","rgb":[1,2,3]});
        let key = coalesce_key(&slider);
        assert_eq!(key.as_deref(), Some("setLightbar"));
        outbox
            .push(
                key.clone(),
                b"a
"
                .to_vec(),
            )
            .unwrap();
        outbox
            .push(
                key,
                b"b
"
                .to_vec(),
            )
            .unwrap();
        assert_eq!(outbox.len(), 1);
        outbox
            .push(
                None,
                b"c
"
                .to_vec(),
            )
            .unwrap();
        assert_eq!(
            outbox.push(
                None,
                b"d
"
                .to_vec()
            ),
            Err(Full)
        );
        outbox.push_force(
            b"e
"
            .to_vec(),
        );
        let first = outbox
            .pop_wait(std::time::Duration::from_millis(1))
            .unwrap();
        assert_eq!(
            first.1,
            b"b
"
        );
        assert!(
            coalesce_key(&serde_json::json!({"action":"setPersonalHaptics","requestId":3}))
                .is_none()
        );
        assert!(coalesce_key(&serde_json::json!({"action":"scan"})).is_none());
        outbox.clear();
        assert!(
            outbox
                .pop_wait(std::time::Duration::from_millis(1))
                .is_none()
        );
    }
}
