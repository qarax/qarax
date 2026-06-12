use std::path::Path;
use std::sync::Arc;

use common::architecture::current_architecture;
use secrecy::{ExposeSecret, Secret};
use sqlx::postgres::{PgConnectOptions, PgSslMode};

#[derive(serde::Deserialize, Debug)]
pub struct ApplicationSettings {
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct SchedulingSettings {
    #[serde(default = "default_memory_oversubscription_ratio")]
    pub memory_oversubscription_ratio: f64,
    #[serde(default = "default_cpu_oversubscription_ratio")]
    pub cpu_oversubscription_ratio: f64,
    #[serde(default = "default_disk_headroom_bytes")]
    pub disk_headroom_bytes: i64,
    #[serde(default = "default_memory_health_floor_bytes")]
    pub memory_health_floor_bytes: i64,
}

impl Default for SchedulingSettings {
    fn default() -> Self {
        Self {
            memory_oversubscription_ratio: default_memory_oversubscription_ratio(),
            cpu_oversubscription_ratio: default_cpu_oversubscription_ratio(),
            disk_headroom_bytes: default_disk_headroom_bytes(),
            memory_health_floor_bytes: default_memory_health_floor_bytes(),
        }
    }
}

fn default_memory_oversubscription_ratio() -> f64 {
    1.0
}

fn default_cpu_oversubscription_ratio() -> f64 {
    4.0
}

fn default_disk_headroom_bytes() -> i64 {
    10 * 1024 * 1024 * 1024
}

fn default_memory_health_floor_bytes() -> i64 {
    1024 * 1024 * 1024
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct HaSettings {
    /// Master switch for the HA failover monitor.
    #[serde(default = "default_ha_enabled")]
    pub enabled: bool,
    /// How long a host must stay down and unreachable before its HA VMs are
    /// failed over. Acts as the split-brain guard: keep it comfortably above
    /// transient network-blip duration.
    #[serde(default = "default_ha_failover_grace_seconds")]
    pub failover_grace_seconds: u64,
    /// How often the HA monitor re-probes down hosts.
    #[serde(default = "default_ha_check_interval_seconds")]
    pub check_interval_seconds: u64,
    /// Minimum delay between failover attempts for the same VM (e.g. while
    /// waiting for capacity to free up).
    #[serde(default = "default_ha_retry_cooldown_seconds")]
    pub retry_cooldown_seconds: u64,
}

impl Default for HaSettings {
    fn default() -> Self {
        Self {
            enabled: default_ha_enabled(),
            failover_grace_seconds: default_ha_failover_grace_seconds(),
            check_interval_seconds: default_ha_check_interval_seconds(),
            retry_cooldown_seconds: default_ha_retry_cooldown_seconds(),
        }
    }
}

fn default_ha_enabled() -> bool {
    true
}

fn default_ha_failover_grace_seconds() -> u64 {
    60
}

fn default_ha_check_interval_seconds() -> u64 {
    15
}

fn default_ha_retry_cooldown_seconds() -> u64 {
    120
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, utoipa::ToSchema)]
pub struct SandboxSettings {
    #[serde(default = "default_sandbox_idle_timeout_secs")]
    pub default_idle_timeout_secs: i32,
    #[serde(default = "default_sandbox_reaper_interval_secs")]
    pub reaper_interval_secs: u64,
    #[serde(default = "default_sandbox_pool_manager_interval_secs")]
    pub pool_manager_interval_secs: u64,
    #[serde(default = "default_sandbox_ready_watcher_interval_secs")]
    pub ready_watcher_interval_secs: u64,
    #[serde(default = "default_sandbox_ready_watcher_max_attempts")]
    pub ready_watcher_max_attempts: u32,
    #[serde(default = "default_sandbox_pool_member_idle_timeout_secs")]
    pub pool_member_idle_timeout_secs: i32,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            default_idle_timeout_secs: default_sandbox_idle_timeout_secs(),
            reaper_interval_secs: default_sandbox_reaper_interval_secs(),
            pool_manager_interval_secs: default_sandbox_pool_manager_interval_secs(),
            ready_watcher_interval_secs: default_sandbox_ready_watcher_interval_secs(),
            ready_watcher_max_attempts: default_sandbox_ready_watcher_max_attempts(),
            pool_member_idle_timeout_secs: default_sandbox_pool_member_idle_timeout_secs(),
        }
    }
}

fn default_sandbox_idle_timeout_secs() -> i32 {
    300
}

fn default_sandbox_reaper_interval_secs() -> u64 {
    10
}

fn default_sandbox_pool_manager_interval_secs() -> u64 {
    10
}

fn default_sandbox_ready_watcher_interval_secs() -> u64 {
    2
}

fn default_sandbox_ready_watcher_max_attempts() -> u32 {
    150
}

fn default_sandbox_pool_member_idle_timeout_secs() -> i32 {
    300
}

#[derive(serde::Deserialize, Debug)]
struct VmDefaultsSettingsRaw {
    pub kernel: String,
    pub firmware: Option<String>,
    pub initramfs: Option<String>,
    pub cmdline: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(from = "VmDefaultsSettingsRaw")]
pub struct VmDefaultsSettings {
    pub kernel: Arc<str>,
    pub firmware: Option<Arc<str>>,
    pub initramfs: Option<Arc<str>>,
    pub cmdline: Arc<str>,
}

impl From<VmDefaultsSettingsRaw> for VmDefaultsSettings {
    fn from(raw: VmDefaultsSettingsRaw) -> Self {
        Self {
            kernel: Arc::from(raw.kernel),
            firmware: raw.firmware.map(Arc::from),
            initramfs: raw.initramfs.map(Arc::from),
            cmdline: Arc::from(raw.cmdline),
        }
    }
}

#[derive(serde::Deserialize, Debug, Default)]
pub struct TelemetrySettings {
    /// Enable OpenTelemetry export (requires the `otel` feature at compile time)
    #[serde(default)]
    pub otel_enabled: bool,
    /// OTLP HTTP endpoint (default: http://localhost:4318).
    /// Overridden by OTEL_EXPORTER_OTLP_ENDPOINT env var.
    #[serde(default = "default_otlp_endpoint")]
    pub otlp_endpoint: String,
}

fn default_otlp_endpoint() -> String {
    "http://localhost:4318".to_string()
}

#[derive(serde::Deserialize, Debug)]
pub struct Settings {
    pub database: DatabaseSettings,
    pub application: ApplicationSettings,
    pub vm_defaults: VmDefaultsSettings,
    #[serde(default)]
    pub scheduling: SchedulingSettings,
    #[serde(default)]
    pub sandbox: SandboxSettings,
    #[serde(default)]
    pub ha: HaSettings,
    #[serde(default)]
    pub telemetry: TelemetrySettings,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct DatabaseSettings {
    pub username: String,
    pub password: Secret<String>,
    pub port: u16,
    pub host: String,

    #[serde(rename = "database_name")]
    pub name: String,

    pub max_connections: u32,
}

impl DatabaseSettings {
    pub fn without_db(&self) -> PgConnectOptions {
        PgConnectOptions::new()
            .host(&self.host)
            .username(&self.username)
            .password(self.password.expose_secret())
            .port(self.port)
            // TODO change at some point
            .ssl_mode(PgSslMode::Prefer)
    }

    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username,
            self.password.expose_secret(),
            self.host,
            self.port,
            self.name
        )
    }

    pub fn connection_string_without_db(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}",
            self.username,
            self.password.expose_secret(),
            self.host,
            self.port
        )
    }
}

pub fn get_configuration() -> Result<Settings, config::ConfigError> {
    // Prefer QARAX_CONFIG_DIR for runtime override (e.g. Docker where CARGO_MANIFEST_DIR is build-path)
    let configuration_directory = if let Ok(dir) = std::env::var("QARAX_CONFIG_DIR") {
        Path::new(&dir).to_path_buf()
    } else {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/..")).join("configuration")
    };
    let environment: Environment = std::env::var("APP_ENVIRONMENT")
        .unwrap_or_else(|_| "local".into())
        .try_into()
        .expect("Failed to parse APP_ENVIRONMENT.");
    let environment_filename = format!("{}.yaml", environment.as_str());
    let settings = config::Config::builder()
        .add_source(config::File::from(
            configuration_directory.join("base.yaml"),
        ))
        .add_source(config::File::from(
            configuration_directory.join(environment_filename),
        ))
        // Override database settings from environment variables if set
        .set_override_option("database.host", std::env::var("DATABASE_HOST").ok())?
        .set_override_option("database.port", std::env::var("DATABASE_PORT").ok())?
        .set_override_option("database.username", std::env::var("DATABASE_USERNAME").ok())?
        .set_override_option("database.password", std::env::var("DATABASE_PASSWORD").ok())?
        .set_override_option(
            "database.database_name",
            std::env::var("DATABASE_NAME").ok(),
        )?
        // Override vm_defaults from environment variables if set and non-empty
        // (empty string means "not set" — fall back to yaml defaults)
        .set_override_option(
            "vm_defaults.kernel",
            std::env::var("VM_KERNEL").ok().filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "vm_defaults.initramfs",
            std::env::var("VM_INITRAMFS").ok().filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "vm_defaults.cmdline",
            std::env::var("VM_CMDLINE").ok().filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "vm_defaults.firmware",
            std::env::var("VM_FIRMWARE").ok().filter(|s| !s.is_empty()),
        )?
        // Override scheduling settings from environment variables if set and non-empty
        .set_override_option(
            "scheduling.disk_headroom_bytes",
            std::env::var("SCHEDULING_DISK_HEADROOM_BYTES")
                .ok()
                .filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "scheduling.memory_health_floor_bytes",
            std::env::var("SCHEDULING_MEMORY_HEALTH_FLOOR_BYTES")
                .ok()
                .filter(|s| !s.is_empty()),
        )?
        // Override HA settings from environment variables if set and non-empty
        .set_override_option(
            "ha.enabled",
            std::env::var("HA_ENABLED").ok().filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "ha.failover_grace_seconds",
            std::env::var("HA_FAILOVER_GRACE_SECONDS")
                .ok()
                .filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "ha.check_interval_seconds",
            std::env::var("HA_CHECK_INTERVAL_SECONDS")
                .ok()
                .filter(|s| !s.is_empty()),
        )?
        // Override telemetry settings from environment variables
        .set_override_option(
            "telemetry.otel_enabled",
            std::env::var("OTEL_ENABLED").ok().filter(|s| !s.is_empty()),
        )?
        .set_override_option(
            "telemetry.otlp_endpoint",
            std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .ok()
                .filter(|s| !s.is_empty()),
        )?
        .build()?;
    settings.try_deserialize::<Settings>()
}

pub fn default_control_plane_architecture() -> String {
    current_architecture()
}
pub enum Environment {
    Local,
    Production,
}

impl Environment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Local => "local",
            Environment::Production => "production",
        }
    }
}

impl TryFrom<String> for Environment {
    type Error = String;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "production" => Ok(Self::Production),
            other => Err(format!(
                "{} is not a supported environment. Use either `local` or `production`.",
                other
            )),
        }
    }
}
