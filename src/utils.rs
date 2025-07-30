use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::{fs, io};

// Load public certificate from file.
pub fn load_certs(filename: &str) -> io::Result<Vec<CertificateDer<'static>>> {
    let certfile = fs::File::open(filename)
        .map_err(|e| io::Error::other(format!("failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(certfile);
    rustls_pemfile::certs(&mut reader).collect()
}

// Load private key from file.
pub fn load_private_key(filename: &str) -> io::Result<PrivateKeyDer<'static>> {
    let keyfile = fs::File::open(filename)
        .map_err(|e| io::Error::other(format!("failed to open {filename}: {e}")))?;
    let mut reader = io::BufReader::new(keyfile);
    rustls_pemfile::private_key(&mut reader).map(|key| key.unwrap())
}
