use secrecy::{ExposeSecret, SecretString};
use serde::Serializer;

pub fn serialize_secret_string<S>(value: &SecretString, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s_val = value.expose_secret();
    s.serialize_str(s_val.as_str())
}

pub fn get_os() -> &'static str {
    let os = std::env::consts::OS;
    os
}

pub fn get_arch() -> &'static str {
    let arch = match std::env::consts::ARCH {
        val if val == "x86_64" => "x64",
        val if val == "aarch64" => "arm64",
        val => val,
    };
    arch
}

pub fn get_os_arch() -> String {
    let os = get_os();
    let arch = get_arch();

    format!("{os}-{arch}")
}
