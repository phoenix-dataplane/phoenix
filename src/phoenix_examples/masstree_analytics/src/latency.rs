//! A Rust re-implementation of eRPC's latency tool.
//! https://github.com/erpc-io/eRPC/blob/v0.2/src/util/latency.h

use std::fmt;
use std::ops::{Index, IndexMut};

pub const BUCKET_CAP: usize = 128;

#[derive(Debug, Clone)]
struct Bucket {
    start: usize,
    step: usize,
    bin: [usize; BUCKET_CAP],
}

impl Bucket {
    const CAP: usize = BUCKET_CAP;
}

impl fmt::Display for Bucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, x) in self.bin.into_iter().enumerate() {
            if x > 0 {
                writeln!(f, "{:4} {:6}", self.val(i), x)?;
            }
        }
        Ok(())
    }
}

impl Index<usize> for Bucket {
    type Output = usize;
    fn index(&self, us: usize) -> &Self::Output {
        debug_assert!(self.contains(us));
        // SAFETY: it is fine because we have already done boundary check before.
        unsafe { self.get_unchecked(us) }
    }
}

impl IndexMut<usize> for Bucket {
    fn index_mut(&mut self, us: usize) -> &mut Self::Output {
        debug_assert!(self.contains(us));
        // SAFETY: it is fine because we have already done boundary check before.
        unsafe { self.get_unchecked_mut(us) }
    }
}

impl Bucket {
    #[inline]
    const fn new(start: usize, step: usize) -> Self {
        Self {
            start,
            step,
            bin: [0; Self::CAP],
        }
    }

    #[inline]
    fn clear(&mut self) {
        self.bin.fill(0);
    }

    #[inline]
    const fn contains(&self, us: usize) -> bool {
        self.start() <= us && us < self.end()
    }

    #[inline]
    const fn start(&self) -> usize {
        self.start
    }

    #[inline]
    const fn end(&self) -> usize {
        self.start + Self::CAP * self.step
    }

    #[inline]
    unsafe fn get_unchecked(&self, us: usize) -> &usize {
        self.bin.get_unchecked((us - self.start) / self.step)
    }

    #[inline]
    unsafe fn get_unchecked_mut(&mut self, us: usize) -> &mut usize {
        self.bin.get_unchecked_mut((us - self.start) / self.step)
    }

    #[inline]
    const fn val(&self, index: usize) -> usize {
        self.start + index * self.step
    }

    #[inline]
    fn reduce_sum(&mut self, other: &Self) {
        for (x, y) in self.bin.iter_mut().zip(other.bin) {
            *x += y;
        }
    }

    #[inline]
    fn count(&self) -> usize {
        self.bin.into_iter().sum()
    }

    #[inline]
    fn sum(&self) -> usize {
        self.bin
            .into_iter()
            .enumerate()
            .fold(0, |accum, (i, x)| accum + x * self.val(i))
    }

    #[inline]
    fn min(&self) -> Option<usize> {
        self.bin.into_iter().enumerate().find_map(
            |(i, x)| {
                if x > 0 {
                    Some(self.val(i))
                } else {
                    None
                }
            },
        )
    }

    #[inline]
    fn max(&self) -> Option<usize> {
        self.bin.into_iter().enumerate().rev().find_map(|(i, x)| {
            if x > 0 {
                Some(self.val(i))
            } else {
                None
            }
        })
    }

    #[inline]
    fn rank(&self, mut rank: usize) -> Option<usize> {
        self.bin.into_iter().enumerate().find_map(|(i, x)| {
            if rank <= x && x > 0 {
                Some(self.val(i))
            } else {
                rank -= x;
                None
            }
        })
    }
}

/// Fast but approximate latency distribution measurement for latency
/// values up to 4000 microseconds (i.e., 4 ms). Adding a latency sample is
/// fast, but computing a statistic is slow.
#[derive(Debug, Clone)]
pub struct Latency {
    // [0, 128) us
    bin0: Bucket,
    // [128, 384) us
    bin1: Bucket,
    // [384, 896) us
    bin2: Bucket,
    // [896, 1920) us
    bin3: Bucket,
    // [1920, 3968) us
    bin4: Bucket,
    // [3968, inf) us
    bin5: usize,
}

impl Default for Latency {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Latency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(writeln!(f, "{}", self.$bin)?;)*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        writeln!(f, "{:4} {:6}", self.bin4.end(), self.bin5)?;
        Ok(())
    }
}

impl Latency {
    #[inline]
    pub const fn new() -> Self {
        #[allow(clippy::identity_op, clippy::erasing_op)]
        Self {
            bin0: Bucket::new(128 * 0, 1 << 0),
            bin1: Bucket::new(128 * 1, 1 << 1),
            bin2: Bucket::new(128 * 3, 1 << 2),
            bin3: Bucket::new(128 * 7, 1 << 3),
            bin4: Bucket::new(128 * 15, 1 << 4),
            bin5: 0,
        }
    }

    #[inline]
    pub fn reset(&mut self) {
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(self.$bin.clear();)*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        self.bin5 = 0;
    }

    /// Add a latency sample
    #[inline]
    pub fn update(&mut self, us: usize) {
        if self.bin0.contains(us) {
            self.bin0[us] += 1;
        } else if self.bin1.contains(us) {
            self.bin1[us] += 1;
        } else if self.bin2.contains(us) {
            self.bin2[us] += 1;
        } else if self.bin3.contains(us) {
            self.bin3[us] += 1;
        } else if self.bin4.contains(us) {
            self.bin4[us] += 1;
        } else {
            self.bin5 += 1
        }
    }

    /// Combine two distributions
    #[inline]
    pub fn combine(&mut self, other: &Self) {
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(self.$bin.reduce_sum(&other.$bin);)*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        self.bin5 += other.bin5;
    }

    /// Return the total number of samples
    pub fn count(&self) -> usize {
        let mut count = self.bin5;
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(count += self.$bin.count();)*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        count
    }

    /// Return the (approximate) sum of all samples
    pub fn sum(&self) -> usize {
        let mut sum = self.bin5 * self.bin4.end();
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(sum += self.$bin.sum();)*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        sum
    }

    /// Return the (approximate) average sample
    pub fn avg(&self) -> f64 {
        self.sum() as f64 / self.count().max(1) as f64
    }

    /// Return the (approximate) minimum sample
    pub fn min(&self) -> usize {
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(
                    if let Some(m) = self.$bin.min() {
                        return m;
                    }
                )*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        self.bin4.end()
    }

    /// Return the (approximate) max sample
    pub fn max(&self) -> usize {
        if self.bin5 > 0 {
            return self.bin4.end();
        }
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(
                    if let Some(m) = self.$bin.min() {
                        return m;
                    }
                )*
            };
        }
        unfold!(bin4, bin3, bin2, bin1, bin0);
        0
    }

    /// Return the (approximate) p-th percentile sample
    pub fn perc(&self, p: f64) -> usize {
        let mut thres = (p * self.count() as f64) as usize;
        macro_rules! unfold {
            ($($bin:ident),*) => {
                $(
                    if let Some(us) = self.$bin.rank(thres) {
                        return us;
                    }
                    thres -= self.$bin.count();
                )*
            };
        }
        unfold!(bin0, bin1, bin2, bin3, bin4);
        self.bin4.end()
    }
}
