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

const RENEWAL_THRESHOLD_DAYS: i64 = 30;

pub fn leaf_cert_needs_renewal(pem_bytes: &[u8]) -> bool {
    let (_, pem) = match x509_parser::pem::parse_x509_pem(pem_bytes) {
        Ok(r) => r,
        Err(_) => return true,
    };
    let cert = match pem.parse_x509() {
        Ok(c) => c,
        Err(_) => return true,
    };
    let not_after_dt = cert.validity().not_after.to_datetime();
    let threshold = OffsetDateTime::now_utc() + Duration::days(RENEWAL_THRESHOLD_DAYS);
    not_after_dt <= threshold
}

pub fn ensure_leaf_cert(domain: &str) -> Result<(String, String), Box<dyn std::error::Error>> {
    let dir = certs_dir();
    let cert_path = dir.join(format!("{domain}.pem"));
    let key_path = dir.join(format!("{domain}.key"));

    if cert_path.exists() && key_path.exists() {
        let pem_bytes = fs::read(&cert_path)?;
        if !leaf_cert_needs_renewal(&pem_bytes) {
            let cert_pem = String::from_utf8(pem_bytes)?;
            let key_pem = fs::read_to_string(&key_path)?;
            return Ok((cert_pem, key_pem));
        }
    }

    generate_leaf_cert(domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cert_pem(not_after: OffsetDateTime) -> String {
        let key_pair = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let mut params = CertificateParams::new(vec!["test.test".to_string()]).unwrap();
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = not_after;
        let cert = params.self_signed(&key_pair).unwrap();
        cert.pem()
    }

    #[test]
    fn cert_expiring_in_10_days_needs_renewal() {
        let pem = make_cert_pem(OffsetDateTime::now_utc() + Duration::days(10));
        assert!(leaf_cert_needs_renewal(pem.as_bytes()));
    }

    #[test]
    fn cert_expiring_in_29_days_needs_renewal() {
        let pem = make_cert_pem(OffsetDateTime::now_utc() + Duration::days(29));
        assert!(leaf_cert_needs_renewal(pem.as_bytes()));
    }

    #[test]
    fn cert_expiring_in_31_days_does_not_need_renewal() {
        let pem = make_cert_pem(OffsetDateTime::now_utc() + Duration::days(31));
        assert!(!leaf_cert_needs_renewal(pem.as_bytes()));
    }

    #[test]
    fn cert_expiring_in_800_days_does_not_need_renewal() {
        let pem = make_cert_pem(OffsetDateTime::now_utc() + Duration::days(800));
        assert!(!leaf_cert_needs_renewal(pem.as_bytes()));
    }

    #[test]
    fn already_expired_cert_needs_renewal() {
        let pem = make_cert_pem(OffsetDateTime::now_utc() - Duration::days(1));
        assert!(leaf_cert_needs_renewal(pem.as_bytes()));
    }

    #[test]
    fn invalid_pem_needs_renewal() {
        assert!(leaf_cert_needs_renewal(b"not a cert"));
    }

    #[test]
    fn empty_bytes_needs_renewal() {
        assert!(leaf_cert_needs_renewal(b""));
    }
}
