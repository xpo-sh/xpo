use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    PKCS_ECDSA_P256_SHA256,
};
use std::fs;
use std::path::PathBuf;
use time::{Duration, OffsetDateTime};

pub fn ca_dir() -> PathBuf {
    xpo_core::config::Config::dir().join("ca")
}

#[allow(dead_code)] // used in Step 3: xpo dev <port>
pub fn certs_dir() -> PathBuf {
    ca_dir().join("certs")
}

pub fn ca_cert_path() -> PathBuf {
    ca_dir().join("root.pem")
}

pub fn ca_key_path() -> PathBuf {
    ca_dir().join("root.key")
}

pub fn ca_exists() -> bool {
    ca_cert_path().exists() && ca_key_path().exists()
}

pub fn generate_ca() -> Result<(), Box<dyn std::error::Error>> {
    let dir = ca_dir();
    fs::create_dir_all(&dir)?;

    let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;

    let mut params = CertificateParams::default();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "xpo.sh Development CA");
    dn.push(DnType::OrganizationName, "xpo.sh");
    params.distinguished_name = dn;

    params.not_before = OffsetDateTime::now_utc();
    params.not_after = OffsetDateTime::now_utc() + Duration::days(3650);

    let cert = params.self_signed(&key_pair)?;

    fs::write(ca_cert_path(), cert.pem())?;
    fs::write(ca_key_path(), key_pair.serialize_pem())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(ca_key_path(), fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

#[allow(dead_code)] // used in Step 3: xpo dev <port>
pub fn generate_leaf_cert(domain: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let cert_pem = fs::read_to_string(ca_cert_path())?;
    let key_pem = fs::read_to_string(ca_key_path())?;
    let ca_key = KeyPair::from_pem(&key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(&cert_pem, &ca_key)?;

    let leaf_key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;

    let mut params = CertificateParams::new(vec![domain.to_string()])?;
    params.not_before = OffsetDateTime::now_utc();
    params.not_after = OffsetDateTime::now_utc() + Duration::days(825);

    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, domain);
    params.distinguished_name = dn;

    let leaf_cert = params.signed_by(&leaf_key, &issuer)?;

    let dir = certs_dir();
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(format!("{domain}.pem")), leaf_cert.pem())?;
    fs::write(dir.join(format!("{domain}.key")), leaf_key.serialize_pem())?;

    Ok((leaf_cert.pem(), leaf_key.serialize_pem()))
}
