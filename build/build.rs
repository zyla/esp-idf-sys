#[cfg(not(any(feature = "pio", feature = "native")))]
compile_error!("One of the features `pio` or `native` must be selected.");

use anyhow::*;
use regex;
use std::{env, iter::once, path::PathBuf};

use embuild::{bindgen, build, cargo, kconfig, path_buf, utils::OsStrExt};

use common::*;

mod common;

// Note that the feature `native` must come before `pio`. These features are really
// mutually exclusive but that would require that all dependencies specify the same
// feature so instead we prefer the `native` feature over `pio` so that if one package
// specifies it, this overrides the `pio` feature for all other dependencies too.
// See https://doc.rust-lang.org/cargo/reference/features.html#mutually-exclusive-features.
#[cfg(any(feature = "pio", feature = "native"))]
#[cfg_attr(feature = "native", path = "native.rs")]
#[cfg_attr(all(feature = "pio", not(feature = "native")), path = "pio.rs")]
mod build_driver;

fn main() -> anyhow::Result<()> {
    let build_output = build_driver::build()?;

    // We need to restrict the kconfig parameters which are turned into rustc cfg items
    // because otherwise we would be hitting rustc command line restrictions on Windows
    //
    // For now, we take all tristate parameters which are set to true, as well as a few
    // selected string ones, as per below
    //
    // This might change in future
    let kconfig_str_allow = regex::Regex::new(r"IDF_TARGET")?;

    let cfg_args = build::CfgArgs {
        args: build_output
            .kconfig_args
            .filter(|(key, value)| {
                matches!(value, kconfig::Value::Tristate(kconfig::Tristate::True))
                    || kconfig_str_allow.is_match(key)
            })
            .filter_map(|(key, value)| value.to_rustc_cfg("esp_idf", key))
            .collect(),
    };

    let mcu = cfg_args.get("esp_idf_idf_target").ok_or_else(|| {
        anyhow!(
            "Failed to get IDF_TARGET from kconfig. cfgs:\n{:?}",
            cfg_args.args
        )
    })?;

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);

    let header_file = path_buf![
        manifest_dir,
        "src",
        "include",
        if mcu == "esp8266" {
            "esp-8266-rtos-sdk"
        } else {
            "esp-idf"
        },
        "bindings.h"
    ];

    cargo::track_file(&header_file);

    let bindings_file = bindgen::run(
        build_output
            .bindgen
            .builder()?
            .ctypes_prefix("c_types")
            .header(header_file.try_to_str()?)
            .blacklist_function("strtold")
            .blacklist_function("_strtold_r")
            .blacklist_function("esp_eth_mac_new_esp32")
            .clang_args(build_output.components.clang_args())
            .clang_args(vec![
                "-target",
                if mcu == "esp32c3" {
                    // Necessary to pass explicitly, because of https://github.com/rust-lang/rust-bindgen/issues/1555
                    "riscv32"
                } else {
                    // We don't really have a similar issue with Xtensa, but we pass it explicitly as well just in case
                    "xtensa"
                },
            ]),
    )?;

    let cfg_args = build::CfgArgs {
        args: cfg_args
            .args
            .into_iter()
            .chain(EspIdfVersion::parse(bindings_file)?.cfg_args())
            .chain(build_output.components.cfg_args())
            .chain(once(mcu))
            .collect(),
    };

    cfg_args.propagate();
    cfg_args.output();

    // In case other SYS crates need to have access to the ESP-IDF C headers
    build_output.cincl_args.propagate();

    if let Some(link_args) = build_output.link_args {
        link_args.propagate();
    }

    Ok(())
}
