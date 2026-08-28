use bytes::Bytes;
use std::collections::VecDeque;

/// Неограниченный (с потолком) лог всех in-game broadcast-пакетов игры,
/// байт-в-байт совпадающий с живым эфиром. Отдаётся FULL-переджойнеру целиком.
pub struct FullHistory {
    inner: VecDeque<Bytes>,
    cap: usize,
}

impl FullHistory {
    /// Потолок 216_000 пакетов ≈ 90 минут при ~40 пакетах/сек, ~10-32 МБ RAM.
    pub fn new() -> Self {
        Self::new_with_cap(216_000)
    }

    pub fn new_with_cap(cap: usize) -> Self {
        Self {
            inner: VecDeque::with_capacity(4096),
            cap,
        }
    }

    pub fn push(&mut self, pkt: Bytes) {
        if self.cap == 0 {
            return;
        }
        if self.inner.len() >= self.cap {
            self.inner.pop_front();
        }
        self.inner.push_back(pkt);
    }

    /// Все пакеты, начиная с индекса `start` (0 = вся история). `start` за пределом → пусто.
    pub fn snapshot_from(&self, start: u32) -> Vec<Bytes> {
        self.inner.iter().skip(start as usize).cloned().collect()
    }

    pub fn len(&self) -> u32 {
        self.inner.len() as u32
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    pub fn bytes_estimate(&self) -> usize {
        self.inner.iter().map(|b| b.len()).sum()
    }
}

impl Default for FullHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_increments_len() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"a"));
        h.push(Bytes::from_static(b"b"));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn snapshot_from_zero_returns_all_in_order() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        let s = h.snapshot_from(0);
        assert_eq!(s, vec![Bytes::from_static(b"1"), Bytes::from_static(b"2")]);
    }

    #[test]
    fn snapshot_from_cursor_skips_prefix() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.snapshot_from(2), vec![Bytes::from_static(b"3")]);
        assert_eq!(h.snapshot_from(3), Vec::<Bytes>::new());
    }

    #[test]
    fn cap_evicts_oldest() {
        let mut h = FullHistory::new_with_cap(2);
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.len(), 2);
        assert_eq!(h.snapshot_from(0)[0], Bytes::from_static(b"2"));
    }

    #[test]
    fn zero_cap_never_retains() {
        let mut h = FullHistory::new_with_cap(0);
        h.push(Bytes::from_static(b"x"));
        assert_eq!(h.len(), 0);
        assert!(h.is_empty());
    }
}
