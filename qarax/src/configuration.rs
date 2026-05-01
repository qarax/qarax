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
