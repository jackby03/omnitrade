//! Safe, generic, power-of-two circular buffer with bitmask indexing.
//!
//! The `RingBuffer` is the foundational data structure for streaming indicator
//! calculations. It uses a compile-time assertion to enforce power-of-two
//! capacity, enabling zero-cost wrapping via bitmask instead of modulo.
//!
//! # Examples
//!
//! ```
//! use omnitrade_core::RingBuffer;
//!
//! let mut buf = RingBuffer::<f64, 4>::new();
//! buf.push(1.0);
//! buf.push(2.0);
//! buf.push(3.0);
//! buf.push(4.0);
//! buf.push(5.0); // overwrites 1.0
//!
//! assert_eq!(buf.len(), 4);
//! assert_eq!(buf.get(0), Some(&2.0)); // oldest
//! assert_eq!(buf.get(3), Some(&5.0)); // newest
//! ```

/// A fixed-capacity circular buffer backed by an array.
///
/// `N` **must** be a power of two. This is enforced at compile time via a
/// const assertion. The power-of-two constraint allows index wrapping with
/// a bitmask (`index & (N - 1)`) instead of the modulo operator, which is
/// significantly faster on most architectures.
///
/// The buffer stores up to `N` elements. Once full, pushing a new element
/// overwrites the oldest entry.
pub struct RingBuffer<T, const N: usize> {
    /// Backing storage. Initialized with `Default::default()`.
    data: [T; N],
    /// Index of the next write position (monotonically increasing, wraps via bitmask).
    write_pos: usize,
    /// Number of elements currently stored (saturates at `N`).
    count: usize,
}

// Compile-time power-of-two enforcement.
impl<T: Default + Copy, const N: usize> RingBuffer<T, N> {
    /// Bitmask for wrapping indices. Equal to `N - 1` when `N` is a power of two.
    const MASK: usize = {
        assert!(N > 0, "RingBuffer capacity must be greater than zero");
        assert!(
            N & (N - 1) == 0,
            "RingBuffer capacity must be a power of two"
        );
        N - 1
    };

    /// Creates a new empty `RingBuffer` with all slots initialized to `T::default()`.
    pub fn new() -> Self {
        Self {
            data: [T::default(); N],
            write_pos: 0,
            count: 0,
        }
    }

    /// Pushes a value into the buffer, overwriting the oldest element if full.
    #[inline]
    pub fn push(&mut self, value: T) {
        self.data[self.write_pos & Self::MASK] = value;
        self.write_pos = self.write_pos.wrapping_add(1);
        if self.count < N {
            self.count += 1;
        }
    }

    /// Returns a reference to the element at the given logical index.
    ///
    /// Index `0` is the **oldest** element, index `len() - 1` is the **newest**.
    /// Returns `None` if `index >= len()`.
    #[inline]
    pub fn get(&self, index: usize) -> Option<&T> {
        if index >= self.count {
            return None;
        }
        // The oldest element is at (write_pos - count), wrapping via bitmask.
        let actual = (self.write_pos.wrapping_sub(self.count).wrapping_add(index)) & Self::MASK;
        Some(&self.data[actual])
    }

    /// Returns a reference to the most recently pushed element, or `None` if empty.
    #[inline]
    pub fn last(&self) -> Option<&T> {
        if self.count == 0 {
            None
        } else {
            Some(&self.data[(self.write_pos.wrapping_sub(1)) & Self::MASK])
        }
    }

    /// Returns a reference to the oldest element in the buffer, or `None` if empty.
    #[inline]
    pub fn first(&self) -> Option<&T> {
        self.get(0)
    }

    /// Returns the number of elements currently stored.
    #[inline]
    pub fn len(&self) -> usize {
        self.count
    }

    /// Returns `true` if the buffer contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Returns `true` if the buffer is at full capacity.
    #[inline]
    pub fn is_full(&self) -> bool {
        self.count == N
    }

    /// Returns the fixed capacity of the buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        N
    }

    /// Returns an iterator over the elements from oldest to newest.
    pub fn iter(&self) -> RingBufferIter<'_, T, N> {
        RingBufferIter {
            buffer: self,
            index: 0,
        }
    }

    /// Clears the buffer, resetting all state.
    pub fn clear(&mut self) {
        self.data = [T::default(); N];
        self.write_pos = 0;
        self.count = 0;
    }
}

impl<T: Default + Copy, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Default + Copy + core::fmt::Debug, const N: usize> core::fmt::Debug for RingBuffer<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// Iterator over `RingBuffer` elements from oldest to newest.
pub struct RingBufferIter<'a, T, const N: usize> {
    buffer: &'a RingBuffer<T, N>,
    index: usize,
}

impl<'a, T: Default + Copy, const N: usize> Iterator for RingBufferIter<'a, T, N> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        if self.index >= self.buffer.count {
            return None;
        }
        let val = self.buffer.get(self.index);
        self.index += 1;
        val
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.buffer.count - self.index;
        (remaining, Some(remaining))
    }
}

impl<'a, T: Default + Copy, const N: usize> ExactSizeIterator for RingBufferIter<'a, T, N> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer() {
        let buf = RingBuffer::<f64, 4>::new();
        assert!(buf.is_empty());
        assert!(!buf.is_full());
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.capacity(), 4);
        assert_eq!(buf.last(), None);
        assert_eq!(buf.first(), None);
        assert_eq!(buf.get(0), None);
    }

    #[test]
    fn push_and_get_below_capacity() {
        let mut buf = RingBuffer::<i32, 4>::new();
        buf.push(10);
        buf.push(20);
        buf.push(30);

        assert_eq!(buf.len(), 3);
        assert!(!buf.is_full());
        assert_eq!(buf.get(0), Some(&10)); // oldest
        assert_eq!(buf.get(1), Some(&20));
        assert_eq!(buf.get(2), Some(&30)); // newest
        assert_eq!(buf.get(3), None);
        assert_eq!(buf.first(), Some(&10));
        assert_eq!(buf.last(), Some(&30));
    }

    #[test]
    fn push_at_capacity() {
        let mut buf = RingBuffer::<i32, 4>::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.push(4);

        assert!(buf.is_full());
        assert_eq!(buf.len(), 4);
        assert_eq!(buf.get(0), Some(&1));
        assert_eq!(buf.get(3), Some(&4));
    }

    #[test]
    fn wrap_around() {
        let mut buf = RingBuffer::<i32, 4>::new();
        for i in 1..=6 {
            buf.push(i);
        }
        // After pushing 1,2,3,4,5,6 into capacity-4 buffer:
        // Buffer should contain [3, 4, 5, 6]
        assert!(buf.is_full());
        assert_eq!(buf.len(), 4);
        assert_eq!(buf.get(0), Some(&3)); // oldest
        assert_eq!(buf.get(1), Some(&4));
        assert_eq!(buf.get(2), Some(&5));
        assert_eq!(buf.get(3), Some(&6)); // newest
        assert_eq!(buf.first(), Some(&3));
        assert_eq!(buf.last(), Some(&6));
    }

    #[test]
    fn multiple_wraps() {
        let mut buf = RingBuffer::<u32, 2>::new();
        for i in 0..100 {
            buf.push(i);
        }
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.get(0), Some(&98));
        assert_eq!(buf.get(1), Some(&99));
    }

    #[test]
    fn iter_order() {
        let mut buf = RingBuffer::<i32, 4>::new();
        for i in 1..=6 {
            buf.push(i);
        }
        let collected: Vec<_> = buf.iter().copied().collect();
        assert_eq!(collected, vec![3, 4, 5, 6]);
    }

    #[test]
    fn iter_partial_fill() {
        let mut buf = RingBuffer::<i32, 8>::new();
        buf.push(10);
        buf.push(20);
        let collected: Vec<_> = buf.iter().copied().collect();
        assert_eq!(collected, vec![10, 20]);
    }

    #[test]
    fn iter_empty() {
        let buf = RingBuffer::<i32, 4>::new();
        let collected: Vec<_> = buf.iter().collect();
        assert!(collected.is_empty());
    }

    #[test]
    fn iter_exact_size() {
        let mut buf = RingBuffer::<i32, 4>::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        let iter = buf.iter();
        assert_eq!(iter.len(), 3);
    }

    #[test]
    fn clear() {
        let mut buf = RingBuffer::<i32, 4>::new();
        buf.push(1);
        buf.push(2);
        buf.push(3);
        buf.clear();
        assert!(buf.is_empty());
        assert_eq!(buf.len(), 0);
        assert_eq!(buf.last(), None);
    }

    #[test]
    fn default_trait() {
        let buf: RingBuffer<f64, 4> = RingBuffer::default();
        assert!(buf.is_empty());
    }

    #[test]
    fn debug_trait() {
        let mut buf = RingBuffer::<i32, 4>::new();
        buf.push(1);
        buf.push(2);
        let debug = format!("{:?}", buf);
        assert_eq!(debug, "[1, 2]");
    }

    #[test]
    fn capacity_1() {
        let mut buf = RingBuffer::<i32, 1>::new();
        assert_eq!(buf.capacity(), 1);
        buf.push(42);
        assert_eq!(buf.get(0), Some(&42));
        assert!(buf.is_full());
        buf.push(99);
        assert_eq!(buf.get(0), Some(&99));
        assert_eq!(buf.len(), 1);
    }

    #[test]
    fn large_capacity() {
        let mut buf = RingBuffer::<u64, 1024>::new();
        for i in 0..2048 {
            buf.push(i);
        }
        assert_eq!(buf.len(), 1024);
        assert_eq!(buf.get(0), Some(&1024));
        assert_eq!(buf.last(), Some(&2047));
    }
}
