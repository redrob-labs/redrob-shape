//! PCG32: small, fast, and seedable per pixel so renders are reproducible.

pub struct Pcg {
    state: u64,
    inc: u64,
}

impl Pcg {
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut p = Self { state: 0, inc: (stream << 1) | 1 };
        p.next_u32();
        p.state = p.state.wrapping_add(seed);
        p.next_u32();
        p
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6364136223846793005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Pcg::new(7, 3);
        let mut b = Pcg::new(7, 3);
        for _ in 0..1000 {
            let x = a.next_f32();
            assert_eq!(x, b.next_f32());
            assert!((0.0..1.0).contains(&x));
        }
        let mut c = Pcg::new(8, 3);
        assert_ne!(a.next_u32(), c.next_u32());
    }
}
