//! 设备级 Application Settings 的稳定 facade。

mod application;
mod credential;
mod custom_connection;
mod model;

pub(crate) use application::{ApplicationSettingsApplication, ApplicationSettingsError};
#[cfg(test)]
pub(crate) use credential::CustomCredentialAdapter;
#[cfg(test)]
pub(crate) use custom_connection::CustomProviderSettings;
pub(crate) use custom_connection::{
    CustomProviderConnection, CustomProviderEffort, CustomProviderProtocol,
};
pub(crate) use model::{
    ApplicationSettings, BabysitModelPreference, BabysitProvider, BabysitProviderPreference,
    BabysitProviderSettings, ThemePreference,
};

#[cfg(test)]
#[path = "tests/contract.rs"]
mod tests;
