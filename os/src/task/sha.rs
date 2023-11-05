use core::hash::{Hasher, BuildHasher};
use sha2::{Digest, Sha256};

pub struct Sha256Hasher {
    hasher: Sha256,
}

impl Sha256Hasher {
    pub fn new() -> Sha256Hasher {
        Sha256Hasher { hasher: Sha256::new() }
    }
}

impl Hasher for Sha256Hasher {
    fn finish(&self) -> u64 {
        let mut buf = [0u8; 32];
        buf.copy_from_slice(&self.hasher.clone().finalize());

        let mut res = u64::from(buf[0]);
        for i in 1..8 {
            res |= u64::from(buf[i]) << 8 * i;
        }
        res
    }

    fn write(&mut self, bytes: &[u8]) {
        self.hasher.update(bytes);
    }
}

#[derive(Default)]
pub struct Sha256Builder();

impl BuildHasher for Sha256Builder {
    type Hasher = Sha256Hasher;

    fn build_hasher(&self) -> Self::Hasher {
        Sha256Hasher::new()
    }
}