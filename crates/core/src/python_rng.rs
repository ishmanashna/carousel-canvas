//! CPython-compatible `random.Random` (MT19937 + version-2 int seeding).

const N: usize = 624;
const M: usize = 397;
const MATRIX_A: u32 = 0x9908_b0df;
const UPPER_MASK: u32 = 0x8000_0000;
const LOWER_MASK: u32 = 0x7fff_ffff;

pub struct PythonRandom {
    state: [u32; N],
    index: usize,
}

impl PythonRandom {
    /// Match `random.Random(seed)` for a non-negative `seed` that fits in `u32`.
    pub fn new(seed: u32) -> Self {
        let mut rng = Self {
            state: [0; N],
            index: N,
        };
        // CPython version=2 int seed: little-endian 32-bit words of abs(seed).
        // Zero still uses a one-element key `[0]`.
        rng.init_by_array(&[seed]);
        rng
    }

    fn init_genrand(&mut self, seed: u32) {
        self.state[0] = seed;
        for i in 1..N {
            self.state[i] = 1_812_433_253u32
                .wrapping_mul(self.state[i - 1] ^ (self.state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        self.index = N;
    }

    fn init_by_array(&mut self, init_key: &[u32]) {
        self.init_genrand(1_965_0218);
        let key_length = init_key.len();
        let mut i = 1usize;
        let mut j = 0usize;
        let mut k = N.max(key_length);
        while k > 0 {
            self.state[i] = (self.state[i]
                ^ ((self.state[i - 1] ^ (self.state[i - 1] >> 30)).wrapping_mul(1_664_525)))
            .wrapping_add(init_key[j])
            .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= N {
                self.state[0] = self.state[N - 1];
                i = 1;
            }
            if j >= key_length {
                j = 0;
            }
            k -= 1;
        }
        k = N - 1;
        while k > 0 {
            self.state[i] = (self.state[i]
                ^ ((self.state[i - 1] ^ (self.state[i - 1] >> 30)).wrapping_mul(1_566_083_941)))
            .wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                self.state[0] = self.state[N - 1];
                i = 1;
            }
            k -= 1;
        }
        self.state[0] = 0x8000_0000;
        self.index = N;
    }

    fn twist(&mut self) {
        for i in 0..N {
            let x = (self.state[i] & UPPER_MASK) | (self.state[(i + 1) % N] & LOWER_MASK);
            let mut x_a = x >> 1;
            if x & 1 != 0 {
                x_a ^= MATRIX_A;
            }
            self.state[i] = self.state[(i + M) % N] ^ x_a;
        }
        self.index = 0;
    }

    fn genrand_uint32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    /// Match CPython `Random.random()` — float in [0.0, 1.0).
    #[allow(dead_code)]
    pub fn random(&mut self) -> f64 {
        let a = self.genrand_uint32() >> 5;
        let b = self.genrand_uint32() >> 6;
        ((a as f64) * 67_108_864.0 + (b as f64)) * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Match CPython `getrandbits(k)` for `k <= 32`.
    pub fn getrandbits(&mut self, k: u32) -> u32 {
        assert!(k > 0 && k <= 32);
        self.genrand_uint32() >> (32 - k)
    }

    /// Match CPython `_randbelow` via `getrandbits`.
    pub fn randbelow(&mut self, n: u32) -> u32 {
        assert!(n > 0);
        if n == 1 {
            return 0;
        }
        let k = 32 - n.leading_zeros(); // bit_length
        let mut r = self.getrandbits(k);
        while r >= n {
            r = self.getrandbits(k);
        }
        r
    }

    pub fn randrange(&mut self, n: u32) -> u32 {
        self.randbelow(n)
    }

    pub fn choice_i32(&mut self, options: &[i32]) -> i32 {
        let idx = self.randbelow(options.len() as u32) as usize;
        options[idx]
    }

    /// Match CPython `Random.shuffle` (Fisher–Yates).
    pub fn shuffle_usize(&mut self, items: &mut [usize]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = self.randbelow(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    /// Match CPython `Random.sample(range(n), k)` (partial shuffle).
    pub fn sample_indices(&mut self, population_len: usize, k: usize) -> Vec<usize> {
        assert!(k <= population_len);
        if k == 0 {
            return vec![];
        }
        let n = population_len;
        let mut pool: Vec<usize> = (0..n).collect();
        let mut result = Vec::with_capacity(k);
        for i in 0..k {
            let j = self.randbelow((n - i) as u32) as usize + i;
            pool.swap(i, j);
            result.push(pool[i]);
        }
        result
    }

    /// Match CPython `Random.randint(a, b)` inclusive on both ends.
    pub fn randint(&mut self, a: i32, b: i32) -> i32 {
        if a > b {
            return self.randint(b, a);
        }
        let span = (b - a) as u32 + 1;
        a + self.randbelow(span) as i32
    }

    /// Match CPython `Random.uniform(a, b)`.
    pub fn uniform(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.random()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_seed_0() {
        let mut r = PythonRandom::new(0);
        assert!((r.random() - 0.8444218515250481).abs() < 1e-15);
        let mut r = PythonRandom::new(0);
        assert_eq!(r.randrange(3), 1);
        assert_eq!(r.choice_i32(&[-1, 1, 2, 3]), 3);
    }

    #[test]
    fn matches_python_seed_42() {
        let mut r = PythonRandom::new(42);
        assert!((r.random() - 0.6394267984578837).abs() < 1e-15);
        let mut r = PythonRandom::new(42);
        assert_eq!(r.randrange(3), 2);
        assert_eq!(r.choice_i32(&[-1, 1, 2, 3]), -1);
    }
}
