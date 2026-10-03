//! The only module that uses the network. It exists only in builds with the `net`
//! feature and is only reachable from `drug update`; lookups never get here.

use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use crate::sources::Fetch;

pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    pub fn new() -> Self {
        // No overall body timeout: release files are large and connections can be slow.
        let agent = ureq::Agent::config_builder()
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(60)))
            .user_agent(concat!("drug/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Self { agent }
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Fetch for Http {
    fn download(
        &self,
        url: &str,
        dest: &Path,
        progress: &mut dyn FnMut(u64, Option<u64>),
    ) -> Result<()> {
        let mut response = self
            .agent
            .get(url)
            .call()
            .with_context(|| format!("cannot download {url}"))?;
        let total = response.body().content_length();
        let mut reader = response.body_mut().as_reader();
        let mut out =
            File::create(dest).with_context(|| format!("cannot create {}", dest.display()))?;
        let mut buf = vec![0; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e).with_context(|| format!("download of {url} failed")),
            };
            out.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        out.sync_all()?;
        Ok(())
    }
}
