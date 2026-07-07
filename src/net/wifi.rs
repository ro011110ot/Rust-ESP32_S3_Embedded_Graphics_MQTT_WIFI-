use heapless::{String, Vec};

pub fn mask_credential(s: &str) -> String<128> {
    let len = s.len();
    let mut out = String::new();
    if len <= 5 {
        for (i, c) in s.chars().enumerate() {
            if i == 0 { out.push(c).ok(); } else { out.push('*').ok(); }
        }
    } else {
        for (i, c) in s.chars().enumerate() {
            if i < 3 || i >= len - 2 { out.push(c).ok(); }
            else if i == 3 { out.push('*').ok(); }
        }
        while out.len() < len { out.push('*').ok(); }
    }
    out
}

pub struct WifiCredential {
    pub ssid: &'static str,
    pub password: &'static str,
}

pub fn get_wifi_credentials() -> Vec<WifiCredential, 3> {
    let mut creds: Vec<WifiCredential, 3> = Vec::new();
    macro_rules! push_cred {
        ($idx:expr) => {{
            let ssid = env_or_panic!(concat!("WIFI_SSID_", $idx));
            let pass = env_or_panic!(concat!("WIFI_PASS_", $idx));
            if !ssid.is_empty() {
                creds.push(WifiCredential { ssid, password: pass }).ok();
            }
        }};
    }
    push_cred!("0");
    push_cred!("1");
    push_cred!("2");
    creds
}
