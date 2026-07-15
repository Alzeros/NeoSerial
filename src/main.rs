mod app;
mod buffer;
mod config;
mod connection;
mod logging;
mod util;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    app::run()
}
