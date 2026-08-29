use bytes::Bytes;
use std::collections::VecDeque;

/// Неограниченный (с потолком) лог всех in-game broadcast-пакетов игры,
/// байт-в-байт совпадающий с живым эфиром. Отдаётся FULL-переджойнеру целиком.
pub struct FullHistory {
    inner: VecDeque<Bytes>,
    cap: usize,
    evicted: u64,
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
            evicted: 0,
        }
    }

    pub fn push(&mut self, pkt: Bytes) {
        if self.cap == 0 {
            return;
        }
        if self.inner.len() >= self.cap {
            self.inner.pop_front();
            self.evicted += 1;
        }
        self.inner.push_back(pkt);
    }

    /// Абсолютный индекс самого старого удержанного пакета (== число вытесненных).
    pub fn first_seq(&self) -> u64 {
        self.evicted
    }

    /// Абсолютный индекс, который получит следующий push (== всего когда-либо добавлено).
    pub fn next_seq(&self) -> u64 {
        self.evicted + self.inner.len() as u64
    }

    /// Пакеты, начиная с АБСОЛЮТНОГО индекса `seq`. Если `seq` уже вытеснен
    /// (`seq < first_seq()`) — отдаёт с самого старого удержанного (front).
    pub fn snapshot_from_seq(&self, seq: u64) -> Vec<Bytes> {
        let rel = seq.saturating_sub(self.evicted) as usize;
        self.inner.iter().skip(rel).cloned().collect()
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
        let s = h.snapshot_from_seq(0);
        assert_eq!(s, vec![Bytes::from_static(b"1"), Bytes::from_static(b"2")]);
    }

    #[test]
    fn snapshot_from_cursor_skips_prefix() {
        let mut h = FullHistory::new();
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.snapshot_from_seq(2), vec![Bytes::from_static(b"3")]);
        assert_eq!(h.snapshot_from_seq(3), Vec::<Bytes>::new());
    }

    #[test]
    fn cap_evicts_oldest() {
        let mut h = FullHistory::new_with_cap(2);
        h.push(Bytes::from_static(b"1"));
        h.push(Bytes::from_static(b"2"));
        h.push(Bytes::from_static(b"3"));
        assert_eq!(h.len(), 2);
        assert_eq!(h.first_seq(), 1);
        assert_eq!(h.next_seq(), 3);
        assert_eq!(h.snapshot_from_seq(1)[0], Bytes::from_static(b"2"));
        assert_eq!(h.snapshot_from_seq(0)[0], Bytes::from_static(b"2"));
    }

    #[test]
    fn evicted_advances_first_and_next_seq() {
        let mut h = FullHistory::new_with_cap(2);
        for i in 1u8..=5 {
            h.push(Bytes::copy_from_slice(&[i]));
        }
        assert_eq!(h.first_seq(), 3);
        assert_eq!(h.next_seq(), 5);
        assert_eq!(h.len(), 2);
        let s = h.snapshot_from_seq(3);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0], Bytes::copy_from_slice(&[4]));
        assert_eq!(s[1], Bytes::copy_from_slice(&[5]));
    }

    #[test]
    fn zero_cap_never_retains() {
        let mut h = FullHistory::new_with_cap(0);
        h.push(Bytes::from_static(b"x"));
        assert_eq!(h.len(), 0);
        assert!(h.is_empty());
    }
}
