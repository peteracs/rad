use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context, Result};
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::EnvFilter;
use wtransport::endpoint::IncomingSession;
use wtransport::tls::self_signed::time::OffsetDateTime;
use wtransport::{Endpoint, Identity, ServerConfig};

mod chaos;

use chaos::{ChaosConfig, ChaosVerdict};

const DEFAULT_WEBTRANSPORT_BIND: &str = "127.0.0.1:4433";
const DEFAULT_AUTHORITY_ADDR: &str = "127.0.0.1:8788";
const DEFAULT_UDP_BIND: &str = "127.0.0.1:0";
const MAX_MATCH_PACKET_BYTES: usize = 2048;

// Persisted dev certificate. The proxy reuses one self-signed identity across
// reboots so the browser-pinned SHA-256 fingerprint stays stable, instead of
// rotating on every boot and forcing a client edit. The cert lives next to the
// crate (independent of the launch CWD) so the Vite dev server can read the
// exact same file and derive the hash automatically. Override the directory
// with MOBA_RAD_CERT_DIR, or bypass persistence entirely with
// MOBA_RAD_CERT_PEM / MOBA_RAD_KEY_PEM for a browser-trusted cert.
const DEFAULT_DEV_CERT_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/.dev-certs");
const DEV_CERT_FILE: &str = "localhost.crt";
const DEV_KEY_FILE: &str = "localhost.key";
const DEV_CERT_SANS: [&str; 3] = ["localhost", "127.0.0.1", "::1"];
// W3C WebTransport caps serverCertificateHashes certs at two weeks of validity;
// Chromium rejects anything past that with `certificate unknown`. Mint for the
// full window but rotate one day early so a long-lived proxy never serves a
// cert the browser has already started refusing.
const DEV_CERT_VALIDITY_DAYS: u32 = 14;
const DEV_CERT_ROTATE_AGE: Duration = Duration::from_secs(13 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug)]
struct ProxyConfig {
    webtransport_bind: SocketAddr,
    authority_addr: SocketAddr,
    udp_bind: SocketAddr,
    chaos: ChaosConfig,
}

impl ProxyConfig {
    fn from_env() -> Result<Self> {
        Ok(Self {
            webtransport_bind: socket_addr_from_env(
                "MOBA_RAD_WEBTRANSPORT_BIND",
                DEFAULT_WEBTRANSPORT_BIND,
            )?,
            authority_addr: socket_addr_from_env(
                "MOBA_RAD_AUTHORITY_ADDR",
                DEFAULT_AUTHORITY_ADDR,
            )?,
            udp_bind: socket_addr_from_env("MOBA_RAD_UDP_BIND", DEFAULT_UDP_BIND)?,
            chaos: ChaosConfig::from_env(),
        })
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    let config = Arc::new(ProxyConfig::from_env()?);
    let identity = load_identity().await?;
    if let Some(hash) = certificate_hash_hex(&identity) {
        // A browser silently refuses the QUIC handshake for a self-signed cert
        // unless the client pins this exact SHA-256 hash. The persisted dev cert
        // keeps this value stable across reboots, and the Vite dev server reads
        // the same cert file to inject the hash automatically — so this line is
        // informational. It only changes when the cert rotates (~every 13 days)
        // or when MOBA_RAD_CERT_PEM/KEY_PEM points at a different identity.
        println!("[WebTransport] Certificate SHA-256 fingerprint: {hash}");
        info!("WebTransport cert ready (auto-injected by the Vite dev server).");
        info!("  Manual override: VITE_MOBA_RAD_WEBTRANSPORT_CERT_HASH={hash}");
    }

    let server_config = ServerConfig::builder()
        .with_bind_address(config.webtransport_bind)
        .with_identity(identity)
        .keep_alive_interval(Some(Duration::from_secs(3)))
        .build();
    let server = Endpoint::server(server_config).context("failed to bind WebTransport endpoint")?;
    let next_session_id = AtomicU64::new(1);

    info!(
        webtransport = %config.webtransport_bind,
        authority = %config.authority_addr,
        udp_bind = %config.udp_bind,
        "moba-rad edge proxy ready"
    );

    if config.chaos.is_enabled() {
        warn!(
            latency_ms = config.chaos.latency_ms(),
            jitter_ms = config.chaos.jitter_ms(),
            loss_pct = config.chaos.loss_pct(),
            "network chaos emulation ENABLED — do not use for production traffic"
        );
    }

    loop {
        let incoming = server.accept().await;
        let session_id = next_session_id.fetch_add(1, Ordering::Relaxed);
        let config = Arc::clone(&config);

        tokio::spawn(async move {
            if let Err(error) = handle_session(session_id, incoming, config).await {
                warn!(session_id, error = ?error, "WebTransport session ended");
            }
        });
    }
}

async fn handle_session(
    session_id: u64,
    incoming: IncomingSession,
    config: Arc<ProxyConfig>,
) -> Result<()> {
    let request = incoming
        .await
        .context("failed to receive session request")?;
    info!(
        session_id,
        authority = request.authority(),
        path = request.path(),
        "accepted WebTransport request"
    );

    if request.path() != "/match" {
        warn!(
            session_id,
            path = request.path(),
            "rejecting unexpected WebTransport path"
        );
        request.not_found().await;
        return Ok(());
    }

    let connection = request
        .accept()
        .await
        .context("failed to accept WebTransport session")?;
    let udp = Arc::new(
        UdpSocket::bind(config.udp_bind)
            .await
            .with_context(|| format!("failed to bind local UDP socket {}", config.udp_bind))?,
    );
    udp.connect(config.authority_addr)
        .await
        .with_context(|| format!("failed to connect UDP socket to {}", config.authority_addr))?;

    info!(
        session_id,
        browser = %connection.remote_address(),
        authority = %config.authority_addr,
        udp_local = %udp.local_addr().unwrap_or(config.udp_bind),
        "bridging WebTransport datagrams to RAD UDP authority"
    );

    let browser_to_authority =
        pump_browser_to_authority(session_id, connection.clone(), Arc::clone(&udp), config.chaos);
    let authority_to_browser =
        pump_authority_to_browser(session_id, connection.clone(), Arc::clone(&udp), config.chaos);

    tokio::select! {
        result = browser_to_authority => result,
        result = authority_to_browser => result,
        closed = connection.closed() => {
            debug!(session_id, reason = ?closed, "WebTransport connection closed");
            Ok(())
        }
    }
}

async fn pump_browser_to_authority(
    session_id: u64,
    connection: wtransport::Connection,
    udp: Arc<UdpSocket>,
    chaos: ChaosConfig,
) -> Result<()> {
    let mut rng = chaos::seeded_rng(session_id, 0xB2A_0001);

    loop {
        let datagram = connection
            .receive_datagram()
            .await
            .context("failed to receive WebTransport datagram")?;
        if datagram.is_empty() {
            continue;
        }
        if datagram.len() > MAX_MATCH_PACKET_BYTES {
            warn!(
                session_id,
                bytes = datagram.len(),
                max = MAX_MATCH_PACKET_BYTES,
                "dropping oversized browser packet"
            );
            continue;
        }

        // Production path: forward immediately. Chaos only intercepts when a knob
        // is set, and even then a zero delay falls through to the direct send so
        // ordering is preserved when only loss is configured.
        if chaos.is_enabled() {
            match chaos.decide(&mut rng) {
                ChaosVerdict::Drop => {
                    debug!(session_id, bytes = datagram.len(), "chaos drop browser -> authority");
                    continue;
                }
                ChaosVerdict::Deliver(delay) if !delay.is_zero() => {
                    let udp = Arc::clone(&udp);
                    let bytes = datagram[..].to_vec();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        if let Err(error) = udp.send(&bytes).await {
                            warn!(session_id, error = ?error, "chaos-delayed browser -> authority send failed");
                        }
                    });
                    continue;
                }
                ChaosVerdict::Deliver(_) => {}
            }
        }

        udp.send(&datagram)
            .await
            .context("failed to forward browser packet to RAD UDP authority")?;
        debug!(session_id, bytes = datagram.len(), "browser -> authority");
    }
}

async fn pump_authority_to_browser(
    session_id: u64,
    connection: wtransport::Connection,
    udp: Arc<UdpSocket>,
    chaos: ChaosConfig,
) -> Result<()> {
    let mut buffer = [0_u8; MAX_MATCH_PACKET_BYTES];
    let mut rng = chaos::seeded_rng(session_id, 0xA2B_0002);

    loop {
        let bytes = udp
            .recv(&mut buffer)
            .await
            .context("failed to receive RAD UDP authority packet")?;
        if bytes == 0 {
            continue;
        }

        if let Some(max) = connection.max_datagram_size() {
            if bytes > max {
                warn!(
                    session_id,
                    bytes, max, "dropping authority packet larger than WebTransport datagram limit"
                );
                continue;
            }
        }

        if chaos.is_enabled() {
            match chaos.decide(&mut rng) {
                ChaosVerdict::Drop => {
                    debug!(session_id, bytes, "chaos drop authority -> browser");
                    continue;
                }
                ChaosVerdict::Deliver(delay) if !delay.is_zero() => {
                    let connection = connection.clone();
                    let data = buffer[..bytes].to_vec();
                    tokio::spawn(async move {
                        tokio::time::sleep(delay).await;
                        if let Err(error) = connection.send_datagram(&data) {
                            warn!(session_id, error = ?error, "chaos-delayed authority -> browser send failed");
                        }
                    });
                    continue;
                }
                ChaosVerdict::Deliver(_) => {}
            }
        }

        connection
            .send_datagram(&buffer[..bytes])
            .context("failed to forward RAD authority packet to browser")?;
        debug!(session_id, bytes, "authority -> browser");
    }
}

async fn load_identity() -> Result<Identity> {
    let cert = env::var("MOBA_RAD_CERT_PEM").ok();
    let key = env::var("MOBA_RAD_KEY_PEM").ok();

    match (cert, key) {
        (Some(cert), Some(key)) => Identity::load_pemfiles(&cert, &key).await.with_context(|| {
            format!("failed to load MOBA_RAD_CERT_PEM={cert} and MOBA_RAD_KEY_PEM={key}")
        }),
        (None, None) => load_or_create_dev_identity().await,
        _ => bail!("set both MOBA_RAD_CERT_PEM and MOBA_RAD_KEY_PEM, or neither"),
    }
}

// Reuse a self-signed identity persisted on disk so the browser-pinned hash is
// stable across reboots. Mints a fresh cert only when the files are missing,
// unreadable, or close to the 14-day browser validity cap.
async fn load_or_create_dev_identity() -> Result<Identity> {
    let dir = env::var("MOBA_RAD_CERT_DIR").unwrap_or_else(|_| DEFAULT_DEV_CERT_DIR.to_string());
    let dir = PathBuf::from(dir);
    let cert_path = dir.join(DEV_CERT_FILE);
    let key_path = dir.join(DEV_KEY_FILE);

    if cert_path.is_file() && key_path.is_file() {
        match dev_cert_age(&cert_path) {
            Ok(age) if age < DEV_CERT_ROTATE_AGE => {
                match Identity::load_pemfiles(&cert_path, &key_path).await {
                    Ok(identity) => {
                        info!(
                            cert = %cert_path.display(),
                            age_days = age.as_secs() / 86_400,
                            "loaded persisted WebTransport dev certificate"
                        );
                        return Ok(identity);
                    }
                    Err(error) => warn!(
                        ?error,
                        "persisted WebTransport dev certificate failed to load; regenerating"
                    ),
                }
            }
            Ok(age) => info!(
                age_days = age.as_secs() / 86_400,
                "persisted WebTransport dev certificate is near the 14-day browser limit; rotating"
            ),
            Err(error) => warn!(
                ?error,
                "could not read persisted WebTransport dev certificate age; regenerating"
            ),
        }
    }

    create_dev_identity(&dir, &cert_path, &key_path).await
}

async fn create_dev_identity(dir: &Path, cert_path: &Path, key_path: &Path) -> Result<Identity> {
    fs::create_dir_all(dir)
        .with_context(|| format!("failed to create cert directory {}", dir.display()))?;

    let identity = Identity::self_signed_builder()
        .subject_alt_names(&DEV_CERT_SANS)
        .not_before(OffsetDateTime::now_utc())
        .validity_days(DEV_CERT_VALIDITY_DAYS)
        .build()
        .context("failed to generate self-signed WebTransport identity")?;

    // Drop any stale files before writing so a rotated cert never leaves a
    // mismatched key behind if the second write fails.
    let _ = fs::remove_file(cert_path);
    let _ = fs::remove_file(key_path);

    identity
        .certificate_chain()
        .store_pemfile(cert_path)
        .await
        .with_context(|| format!("failed to write {}", cert_path.display()))?;
    identity
        .private_key()
        .store_secret_pemfile(key_path)
        .await
        .with_context(|| format!("failed to write {}", key_path.display()))?;

    info!(
        cert = %cert_path.display(),
        key = %key_path.display(),
        validity_days = DEV_CERT_VALIDITY_DAYS,
        "generated and persisted new WebTransport dev certificate"
    );
    Ok(identity)
}

fn dev_cert_age(path: &Path) -> Result<Duration> {
    let modified = fs::metadata(path)
        .with_context(|| format!("failed to stat {}", path.display()))?
        .modified()
        .with_context(|| format!("filesystem does not report mtime for {}", path.display()))?;
    Ok(SystemTime::now()
        .duration_since(modified)
        .unwrap_or_default())
}

fn certificate_hash_hex(identity: &Identity) -> Option<String> {
    let certificate = identity.certificate_chain().as_slice().first()?;
    Some(hex(certificate.hash().as_ref()))
}

fn socket_addr_from_env(name: &str, fallback: &str) -> Result<SocketAddr> {
    let value = env::var(name).unwrap_or_else(|_| fallback.to_string());
    value
        .parse()
        .with_context(|| format!("invalid {name} socket address: {value}"))
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }

    out
}

fn init_logging() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}
