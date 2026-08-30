use std::{env, fs};

use secrecy::SecretString;

const ENV_PREFIX: &str = "env://";
const FILE_PREFIX: &str = "file://";

pub trait SecretProvider: Send + Sync {
    fn resolve(&self, credential_ref: &str) -> Result<SecretString, SecretProviderError>;
}

#[derive(Debug, Default)]
pub struct ExternalSecretProvider;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SecretProviderError {
    #[error("credential reference must use the env:// or file:// scheme")]
    UnsupportedScheme,
    #[error("credential reference is invalid")]
    InvalidReference,
    #[error("referenced environment variable is not available")]
    EnvironmentVariableUnavailable,
    #[error("referenced credential file could not be read")]
    FileUnavailable,
    #[error("resolved credential is empty")]
    EmptySecret,
}

impl SecretProvider for ExternalSecretProvider {
    fn resolve(&self, credential_ref: &str) -> Result<SecretString, SecretProviderError> {
        let value = if let Some(variable) = credential_ref.strip_prefix(ENV_PREFIX) {
            if variable.is_empty()
                || !variable
                    .chars()
                    .all(|character| character == '_' || character.is_ascii_alphanumeric())
            {
                return Err(SecretProviderError::InvalidReference);
            }
            env::var(variable).map_err(|_| SecretProviderError::EnvironmentVariableUnavailable)?
        } else if let Some(path) = credential_ref.strip_prefix(FILE_PREFIX) {
            if !path.starts_with('/') {
                return Err(SecretProviderError::InvalidReference);
            }
            fs::read_to_string(path).map_err(|_| SecretProviderError::FileUnavailable)?
        } else {
            return Err(SecretProviderError::UnsupportedScheme);
        };

        let value = value
            .strip_suffix("\r\n")
            .or_else(|| value.strip_suffix('\n'))
            .unwrap_or(&value)
            .to_string();
        if value.is_empty() {
            return Err(SecretProviderError::EmptySecret);
        }
        Ok(SecretString::new(value))
    }
}

pub fn validate_credential_ref(credential_ref: &str) -> Result<(), SecretProviderError> {
    if let Some(variable) = credential_ref.strip_prefix(ENV_PREFIX) {
        if !variable.is_empty()
            && variable
                .chars()
                .all(|character| character == '_' || character.is_ascii_alphanumeric())
        {
            return Ok(());
        }
    } else if credential_ref
        .strip_prefix(FILE_PREFIX)
        .is_some_and(|path| path.starts_with('/') && path.len() > 1)
    {
        return Ok(());
    } else if !credential_ref.contains("://") {
        return Err(SecretProviderError::UnsupportedScheme);
    }
    Err(SecretProviderError::InvalidReference)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use secrecy::ExposeSecret;

    use super::*;

    #[test]
    fn resolves_environment_variables_without_exposing_reference_in_errors() {
        unsafe { env::set_var("QARAX_SECRET_PROVIDER_TEST", "environment-secret") };
        let secret = ExternalSecretProvider
            .resolve("env://QARAX_SECRET_PROVIDER_TEST")
            .unwrap();
        unsafe { env::remove_var("QARAX_SECRET_PROVIDER_TEST") };
        assert_eq!(secret.expose_secret(), "environment-secret");

        let error = ExternalSecretProvider
            .resolve("env://QARAX_SECRET_PROVIDER_MISSING")
            .unwrap_err();
        assert!(!error.to_string().contains("QARAX_SECRET_PROVIDER_MISSING"));
    }

    #[test]
    fn resolves_mounted_files_and_removes_one_trailing_newline() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        write!(file, "file-secret\n").unwrap();
        let credential_ref = format!("file://{}", file.path().display());
        let secret = ExternalSecretProvider.resolve(&credential_ref).unwrap();
        assert_eq!(secret.expose_secret(), "file-secret");
    }

    #[test]
    fn validates_supported_reference_syntax_without_resolving_it() {
        assert!(validate_credential_ref("env://HOST_PASSWORD").is_ok());
        assert!(validate_credential_ref("file:///run/secrets/host-password").is_ok());
        assert!(validate_credential_ref("env://").is_err());
        assert!(validate_credential_ref("file://relative").is_err());
        assert!(validate_credential_ref("vault://secret").is_err());
    }
}
