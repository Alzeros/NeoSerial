use std::collections::VecDeque;

/// 固定容量的环形缓冲。满了 push 新元素时丢弃最旧元素。
pub struct RingBuffer<T> {
    buf: VecDeque<T>,
    capacity: usize,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(capacity: usize) -> Self {
        RingBuffer {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, item: T) {
        if self.buf.len() >= self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(item);
    }

    pub fn push_many(&mut self, items: Vec<T>) {
        for item in items {
            self.push(item);
        }
    }

    /// 返回当前所有元素的快照（从旧到新）。
    pub fn snapshot(&self) -> Vec<T> {
        self.buf.iter().cloned().collect()
    }

    /// 当前最旧元素的引用（空返回 None）。只读队首，不像 snapshot 那样克隆整个缓冲。
    pub fn front(&self) -> Option<&T> {
        self.buf.front()
    }

    /// 当前最新元素的引用（空返回 None）。只读队尾，不克隆。
    pub fn back(&self) -> Option<&T> {
        self.buf.back()
    }

    /// 按插入序（旧→新）迭代。供 RxHistory::since 在锁内扫描过滤，
    /// 避免先 snapshot 全量深拷贝再 filter 的分配风暴。
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.buf.iter()
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[allow(dead_code)]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_under_cap() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        assert_eq!(rb.snapshot(), vec![1, 2]);
        assert_eq!(rb.len(), 2);
    }

    #[test]
    fn test_overflow_drops_oldest() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.push(2);
        rb.push(3);
        rb.push(4); // 1 被丢弃
        assert_eq!(rb.snapshot(), vec![2, 3, 4]);
        assert_eq!(rb.len(), 3);
    }

    #[test]
    fn test_push_many() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push_many(vec![1, 2, 3, 4, 5]);
        assert_eq!(rb.snapshot(), vec![3, 4, 5]);
    }

    #[test]
    fn test_clear() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(3);
        rb.push(1);
        rb.clear();
        assert!(rb.is_empty());
        assert_eq!(rb.snapshot(), Vec::<i32>::new());
    }

    #[test]
    fn test_many_overflow_exact_cap() {
        let mut rb: RingBuffer<i32> = RingBuffer::new(5);
        rb.push_many(vec![1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(rb.snapshot(), vec![3, 4, 5, 6, 7]);
    }
}
