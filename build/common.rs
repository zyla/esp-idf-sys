use anyhow::*;
use regex::{self};
use std::{collections::HashSet, error, fs, iter::once, path::Path, str::FromStr};

use embuild::{bindgen, build, kconfig};

pub const STABLE_PATCHES: &[&str] = &[
    "patches/missing_xtensa_atomics_fix.diff",
    "patches/pthread_destructor_fix.diff",
    "patches/ping_setsockopt_fix.diff",
];

#[allow(unused)]
pub const MASTER_PATCHES: &[&str] = &[];

const ALL_COMPONENTS: &[&str] = &[
    // TODO: Put all IDF components here
    "comp_pthread_enabled",
    "comp_nvs_flash_enabled",
    "comp_esp_http_client_enabled",
    "comp_esp_http_server_enabled",
    "comp_espcoredump_enabled",
    "comp_app_update_enabled",
    "comp_esp_serial_slave_link_enabled",
    "comp_spi_flash_enabled",
    "comp_esp_adc_cal_enabled",
];

pub struct EspIdfBuildOutput {
    pub cincl_args: build::CInclArgs,
    pub link_args: Option<build::LinkArgs>,
    pub kconfig_args: Box<dyn Iterator<Item = (String, kconfig::Value)>>,
    pub components: EspIdfComponents,
    pub bindgen: bindgen::Factory,
}

pub struct EspIdfComponents(Vec<&'static str>);

impl EspIdfComponents {
    pub fn new() -> Self {
        Self(ALL_COMPONENTS.iter().map(|s| *s).collect::<Vec<_>>())
    }

    #[allow(dead_code)]
    pub fn from<I, S>(enabled: I) -> Self
    where
        I: Iterator<Item = S>,
        S: Into<String>,
    {
        let enabled = enabled.map(Into::into).collect::<HashSet<_>>();

        Self(
            ALL_COMPONENTS
                .iter()
                .map(|s| *s)
                .filter(|s| enabled.contains(*s))
                .collect::<Vec<_>>(),
        )
    }

    pub fn clang_args<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        self.0
            .iter()
            .map(|s| format!("-DESP_IDF_{}", s.to_uppercase()))
    }

    pub fn cfg_args<'a>(&'a self) -> impl Iterator<Item = String> + 'a {
        self.0.iter().map(|c| format!("esp_idf_{}", c))
    }
}

pub struct EspIdfVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl EspIdfVersion {
    pub fn parse(bindings_file: impl AsRef<Path>) -> Result<Self> {
        let bindings_content = fs::read_to_string(bindings_file.as_ref())?;

        Ok(Self {
            major: Self::grab_const(&bindings_content, "ESP_IDF_VERSION_MAJOR", "u32")?,
            minor: Self::grab_const(&bindings_content, "ESP_IDF_VERSION_MINOR", "u32")?,
            patch: Self::grab_const(bindings_content, "ESP_IDF_VERSION_PATCH", "u32")?,
        })
    }

    pub fn cfg_args(&self) -> impl Iterator<Item = String> {
        once(format!(
            "esp_idf_full_version=\"{}.{}.{}\"",
            self.major, self.minor, self.patch
        ))
        .chain(once(format!(
            "esp_idf_version=\"{}.{}\"",
            self.major, self.minor
        )))
        .chain(once(format!("esp_idf_major_version=\"{}\"", self.major)))
        .chain(once(format!("esp_idf_minor_version=\"{}\"", self.minor)))
        .chain(once(format!("esp_idf_patch_version=\"{}\"", self.patch)))
    }

    fn grab_const<T>(
        text: impl AsRef<str>,
        const_name: impl AsRef<str>,
        const_type: impl AsRef<str>,
    ) -> Result<T>
    where
        T: FromStr,
        T::Err: error::Error + Send + Sync + 'static,
    {
        // Future: Consider using bindgen::callbacks::ParseCallbacks for grabbing macro-based constants. Should be more reliable compared to grepping

        let const_name = const_name.as_ref();

        let value = regex::Regex::new(&format!(
            r"\s+const\s+{}\s*:\s*{}\s*=\s*(\S+)\s*;",
            const_name,
            const_type.as_ref()
        ))?
        .captures(text.as_ref())
        .ok_or_else(|| anyhow!("Failed to capture constant {}", const_name))?
        .get(1)
        .ok_or_else(|| anyhow!("Failed to capture the value of constant {}", const_name))?
        .as_str()
        .parse::<T>()?;

        Ok(value)
    }
}
