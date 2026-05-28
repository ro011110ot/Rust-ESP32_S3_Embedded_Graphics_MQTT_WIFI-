/// build.rs — Compile-time environment variable injection from .env
///
/// This build script reads the `.env` file at compile time and exports every
/// variable via `cargo:rustc-env=KEY=VALUE`.  This allows the application to
/// access secrets (WiFi SSID/password, MQTT credentials, API keys) through
/// the `env!()` macro without embedding them in the source code.
///
/// Security: the .env file is listed in .gitignore and must NEVER be
/// committed.  The companion .env_TEMPLATE file shows required keys with
/// empty values so other developers know what to configure.
///
/// How to use in source code:
///     let ssid = env!("WIFI_SSID_0");
///     let pass = env!("WIFI_PASS_0");

use std::path::Path;

fn main() {
    let env_path = Path::new(".env");

    // Only proceed if .env exists — the template is not enough
    if !env_path.exists() {
        println!("cargo:warning=No .env file found. Copy .env_TEMPLATE to .env and fill in values.");
        return;
    }

    let content = std::fs::read_to_string(env_path)
        .expect("Failed to read .env file");

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();

        // Skip blank lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Parse KEY=VALUE pairs — split only on the first '='
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            // Strip optional surrounding quotes from the value
            let value = value.trim().trim_matches('"').trim_matches('\'');

            if !key.is_empty() {
                println!("cargo:rustc-env={}={}", key, value);
            } else {
                println!("cargo:warning=Empty key on line {} in .env", line_num + 1);
            }
        } else {
            println!(
                "cargo:warning=Malformed line {} in .env (no '=' found)",
                line_num + 1
            );
        }
    }

    // Re-run build.rs whenever .env changes
    println!("cargo:rerun-if-changed=.env");
}
