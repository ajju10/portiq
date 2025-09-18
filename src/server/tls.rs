use crate::config::TLSConfig;
use crate::utils::{load_certs, load_private_key};
use rustls::crypto::aws_lc_rs::sign::any_supported_type;
use rustls::server::{ClientHello, ResolvesServerCert, ResolvesServerCertUsingSni};
use rustls::sign::CertifiedKey;
use std::sync::Arc;

#[derive(Debug)]
struct SNICertificateResolver {
    default: Arc<CertifiedKey>,
    sni: ResolvesServerCertUsingSni,
}

impl SNICertificateResolver {
    fn new(cert_file: &str, key_file: &str) -> Self {
        let certs = load_certs(cert_file).unwrap();
        let private_key = load_private_key(key_file).unwrap();
        let signing_key = any_supported_type(&private_key).unwrap();
        SNICertificateResolver {
            default: Arc::new(CertifiedKey::new(certs, signing_key)),
            sni: ResolvesServerCertUsingSni::new(),
        }
    }

    fn add_sni_cert(
        &mut self,
        hostname: &str,
        cert_file: &str,
        key_file: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let certs = load_certs(cert_file)?;
        let private_key = load_private_key(key_file)?;
        let signing_key = any_supported_type(&private_key)?;
        self.sni
            .add(hostname, CertifiedKey::new(certs, signing_key))?;
        Ok(())
    }
}

impl ResolvesServerCert for SNICertificateResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.sni
            .resolve(client_hello)
            .or_else(|| Some(self.default.clone()))
    }
}

pub fn init_rustls_server_config(tls_configs: &[TLSConfig]) -> Arc<rustls::ServerConfig> {
    let default_cfg = tls_configs
        .iter()
        .find(|&cfg| cfg.default)
        .expect("A default config is required for TLS");

    let mut resolver = SNICertificateResolver::new(
        default_cfg.cert_file.to_str().unwrap(),
        default_cfg.key_file.to_str().unwrap(),
    );

    for tls_config in tls_configs {
        if let Some(hosts) = &tls_config.hostnames {
            let cert_file = tls_config.cert_file.to_str().unwrap();
            let key_file = tls_config.key_file.to_str().unwrap();
            for host in hosts {
                resolver
                    .add_sni_cert(host, cert_file, key_file)
                    .unwrap_or_else(|_| {
                        panic!("The certificate should be valid for hostname `{host}`")
                    });
            }
        }
    }

    let mut server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));

    server_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(server_config)
}
