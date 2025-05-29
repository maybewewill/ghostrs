use sha1::{Digest, Sha1};

#[derive(Debug, Default, Clone)]
pub struct SHA1 {
    hasher: Sha1,
    finalized: bool,
    digest: Option<[u8; 20]>,
}

impl SHA1 {
    pub fn new() -> Self {
        SHA1 {
            hasher: Sha1::new(),
            finalized: false,
            digest: None,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        if self.finalized {
            panic!("SHA1 has already been finalized. Reset before reuse.");
        }
        self.hasher.update(data);
    }

    pub fn finalise(&mut self) {
        if !self.finalized {
            let result = self.hasher.clone().finalize();
            self.digest = Some(result.into());
            self.finalized = true;
        }
    }

    pub fn get_hash(&self) -> Option<[u8; 20]> {
        self.digest
    }

    pub fn report_hash(&self, hex: bool) -> String {
        match self.digest {
            Some(digest) => {
                if hex {
                    digest.iter().map(|b| format!("{:02x}", b)).collect::<String>()
                } else {
                    digest.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(" ")
                }
            }
            None => String::from("Error: SHA1 not finalized!"),
        }
    }

    pub fn reset(&mut self) {
        self.hasher = Sha1::new();
        self.finalized = false;
        self.digest = None;
    }

    /// One-shot compute for convenience
    pub fn compute(data: &[u8]) -> [u8; 20] {
        let result = Sha1::digest(data);
        let mut output = [0u8; 20];
        output.copy_from_slice(&result);
        output
    }

    /// Report one-shot in hex or digit form
    pub fn compute_report(data: &[u8], hex: bool) -> String {
        let digest = SHA1::compute(data);
        if hex {
            digest.iter().map(|b| format!("{:02x}", b)).collect::<String>()
        } else {
            digest.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(" ")
        }
    }
}
