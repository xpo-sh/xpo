use crate::config::ServerConfig;
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::server::ResolvesServerCert;
use rustls::sign::CertifiedKey;
use rustls::ServerConfig as RustlsConfig;
use std::sync::{Arc, RwLock};
use tokio_rustls::TlsAcceptor;
use tracing::info;

pub struct CertResolver {
    key: RwLock<Arc<CertifiedKey>>,
}

impl std::fmt::Debug for CertResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CertResolver").finish()
    }
}

impl CertResolver {
    fn new(certified_key: CertifiedKey) -> Self {
        Self {
            key: RwLock::new(Arc::new(certified_key)),
        }
    }

    pub fn update(&self, certs: Vec<CertificateDer<'static>>, key: PrivateKeyDer<'static>) {
        let signing_key =
            rustls::crypto::ring::sign::any_supported_type(&key).expect("invalid signing key");
        let certified_key = CertifiedKey::new(certs, signing_key);
        *self.key.write().unwrap() = Arc::new(certified_key);
    }
}

impl ResolvesServerCert for CertResolver {
    fn resolve(&self, _client_hello: rustls::server::ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        Some(self.key.read().unwrap().clone())
    }
}

pub fn build_tls(config: &ServerConfig) -> Option<(TlsAcceptor, Arc<CertResolver>)> {
    if let (Some(cert_path), Some(key_path)) = (&config.tls_cert, &config.tls_key) {
        info!("loading TLS cert from disk");
        let certs = load_certs(cert_path);
        let key = load_key(key_path);
        Some(make_tls(certs, key))
    } else if config.acme_enabled {
        let cert_path = config.cert_path();
        let key_path = config.key_path();
        if std::path::Path::new(&cert_path).exists() && std::path::Path::new(&key_path).exists() {
            info!("loading ACME cert from disk");
            let certs = load_certs(&cert_path);
            let key = load_key(&key_path);
            Some(make_tls(certs, key))
        } else {
            info!("no cert on disk, ACME will provision before starting TLS");
            None
        }
    } else if config.tls_self_signed {
        info!(domain = %config.base_domain, "generating self-signed TLS cert");
        let (certs, key) = generate_self_signed(&config.base_domain);
        Some(make_tls(certs, key))
    } else {
        None
    }
}

pub fn make_tls(
    certs: Vec<CertificateDer<'static>>,
    key: PrivateKeyDer<'static>,
) -> (TlsAcceptor, Arc<CertResolver>) {
    let signing_key =
        rustls::crypto::ring::sign::any_supported_type(&key).expect("failed to create signing key");
    let certified_key = CertifiedKey::new(certs, signing_key);
    let resolver = Arc::new(CertResolver::new(certified_key));

    let tls_config =
        RustlsConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .expect("failed to set TLS versions")
            .with_no_client_auth()
            .with_cert_resolver(resolver.clone());

    (TlsAcceptor::from(Arc::new(tls_config)), resolver)
}

pub fn load_certs(path: &str) -> Vec<CertificateDer<'static>> {
    let pem = std::fs::read_to_string(path).expect("failed to read TLS cert");
    rustls_pemfile::certs(&mut pem.as_bytes())
        .collect::<Result<Vec<_>, _>>()
        .expect("failed to parse TLS cert")
}

pub fn load_key(path: &str) -> PrivateKeyDer<'static> {
    let pem = std::fs::read_to_string(path).expect("failed to read TLS key");
    rustls_pemfile::private_key(&mut pem.as_bytes())
        .expect("failed to parse TLS key")
        .expect("no private key found in PEM")
}

fn generate_self_signed(
    base_domain: &str,
) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
    let key_pair =
        KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).expect("failed to generate key pair");

    let subjects = vec![format!("*.{base_domain}"), base_domain.to_string()];
    let params = CertificateParams::new(subjects).expect("failed to create cert params");
    let cert = params
        .self_signed(&key_pair)
        .expect("failed to self-sign cert");

    let cert_der = CertificateDer::from(cert.der().to_vec());
    let key_der = PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_pair.serialize_der()));

    (vec![cert_der], key_der)
}
