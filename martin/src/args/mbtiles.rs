use log::info;
use mbtiles::ValidationLevel;

use crate::mbtiles::{MbtConfig, OnInvalid};

#[derive(clap::Args, Debug, PartialEq, Default)]
#[command(about, version)]
pub struct MbtArgs {
    /// Level of validation to apply to .mbtiles
    #[arg(long)]
    pub validate_mbtiles: Option<ValidationLevel>,
    /// How to handle invalid .mbtiles
    #[arg(long)]
    pub on_invalid_mbtiles: Option<OnInvalid>,
}

impl MbtArgs {
    /// Apply CLI parameters from `self` to the configuration loaded from the config file `mbtiles`
    pub fn override_config(self, mbt_config: &mut MbtConfig) {
        // This ensures that if a new parameter is added to the struct, it will not be forgotten here
        let Self {
            validate_mbtiles,
            on_invalid_mbtiles,
        } = self;

        if let Some(value) = validate_mbtiles {
            info!("Overriding configured default mbtiles.validate to {value}");
            mbt_config.validate = validate_mbtiles.unwrap_or_default();
        }

        if let Some(value) = on_invalid_mbtiles {
            info!("Overriding configured default mbtiles.on_invalid to {value}");
            mbt_config.on_invalid = on_invalid_mbtiles.unwrap_or_default();
        }
    }
}
