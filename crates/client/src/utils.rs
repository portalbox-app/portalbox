use std::sync::Arc;

use tokio_rustls::TlsConnector;

pub fn get_tls_connector() -> anyhow::Result<TlsConnector> {
    let mut root_cert_store = tokio_rustls::rustls::RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs()?;
    for cert in native_certs {
        root_cert_store
            .add(&tokio_rustls::rustls::Certificate(cert.0))
            .unwrap();
    }

    let config = tokio_rustls::rustls::ClientConfig::builder()
        .with_safe_defaults()
        .with_root_certificates(root_cert_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));

    Ok(connector)
}
