/// build.rs — Compile-time environment variable injection from .env
/// + libprintf stub generation to avoid dangerous relocation errors
///   from _ftoa (which calls double-precision libgcc functions that
///   cross the 1GB boundary in the default ESP32-S3 link layout).
/// + linker script selection and error handling (from esp-generate template).

use std::path::Path;

fn main() {
    // ── 1. Inject .env into compile-time env!() ──────────────────────
    let env_path = Path::new(".env");

    if !env_path.exists() {
        println!("cargo:warning=No .env file found. Copy .env_TEMPLATE to .env and fill in values.");
        return;
    }

    let content = std::fs::read_to_string(env_path)
        .expect("Failed to read .env file");

    for (line_num, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
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

    println!("cargo:rerun-if-changed=.env");

    // ── 2. Linker script & error handling ────────────────────────────
    linker_be_nice();

    // make sure linkall.x is the last linker script (otherwise might cause
    // problems with flip-link)
    println!("cargo:rustc-link-arg=-Tlinkall.x");

    // ── 3. Generate stub libprintf.a ────────────────────────────────
    let out_dir = std::path::PathBuf::from(
        std::env::var("OUT_DIR").expect("OUT_DIR not set"),
    );

    let cc = std::env::var("CC")
        .unwrap_or_else(|_| "xtensa-esp32s3-elf-gcc".to_string());
    let ar = std::env::var("AR")
        .unwrap_or_else(|_| "xtensa-esp32s3-elf-ar".to_string());

    let stub_c = out_dir.join("printf_stub.c");
    let stub_o = out_dir.join("printf_stub.o");
    let stub_a = out_dir.join("libprintf.a");

    if !stub_a.exists() {
        let c_code = r#"
#include <stdarg.h>
#include <stddef.h>

void _ftoa(void) {}
void _etoa(void) {}

int vsnprintf_(char *s, size_t n, const char *fmt, va_list ap)
    __attribute__((weak));

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) {
    if (vsnprintf_) return vsnprintf_(s, n, fmt, ap);
    return 0;
}
int vsprintf(char *s, const char *fmt, va_list ap) {
    return vsnprintf(s, (size_t)-1, fmt, ap);
}
int sprintf(char *s, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(s, (size_t)-1, fmt, ap);
    va_end(ap); return r;
}
int snprintf(char *s, size_t n, const char *fmt, ...) {
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(s, n, fmt, ap);
    va_end(ap); return r;
}
int printf(const char *fmt, ...) {
    char buf[512];
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap); return r;
}
int vprintf(const char *fmt, va_list ap) {
    char buf[512];
    return vsnprintf(buf, sizeof buf, fmt, ap);
}

int phy_printf(const char *fmt, ...) {
    char buf[512];
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap); return r;
}
int net80211_printf(const char *fmt, ...) {
    char buf[512];
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap); return r;
}
int pp_printf(const char *fmt, ...) {
    char buf[512];
    va_list ap; va_start(ap, fmt);
    int r = vsnprintf(buf, sizeof buf, fmt, ap);
    va_end(ap); return r;
}

__attribute__((weak)) void *memcpy_(void *d, const void *s, size_t n);
void *memcpy(void *d, const void *s, size_t n) {
    if (memcpy_) return memcpy_(d, s, n);
    return d;
}
__attribute__((weak)) void *memset_(void *d, int c, size_t n);
void *memset(void *d, int c, size_t n) {
    if (memset_) return memset_(d, c, n);
    return d;
}
"#;
        std::fs::write(&stub_c, c_code).expect("write printf_stub.c");

        let status = std::process::Command::new(&cc)
            .args(&[
                "-c", "-o",
                stub_o.to_str().unwrap(),
                stub_c.to_str().unwrap(),
                "-nostdlib", "-ffreestanding",
                "-I", "/home/ro011110ot/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/lib/gcc/xtensa-esp-elf/15.2.0/include",
            ])
            .status()
            .expect("failed to compile printf stub");
        assert!(status.success(), "CC compile failed");

        let status = std::process::Command::new(&ar)
            .args(&["rcs",
                stub_a.to_str().unwrap(),
                stub_o.to_str().unwrap(),
            ])
            .status()
            .expect("failed to create libprintf.a");
        assert!(status.success(), "AR failed");
    }

    println!("cargo:rustc-link-search={}", out_dir.display());
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `defmt` not found - make sure `defmt.x` is added as a linker script"
                    );
                    eprintln!();
                }
                "_stack_start" => {
                    eprintln!();
                    eprintln!("💡 Is the linker script `linkall.x` missing?");
                    eprintln!();
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!();
                    eprintln!(
                        "💡 `esp-radio` has no scheduler enabled. Make sure you have initialized `esp-rtos`"
                    );
                    eprintln!();
                }
                "free" | "malloc" | "calloc" | "get_free_internal_heap_size"
                | "malloc_internal" | "realloc_internal" | "calloc_internal"
                | "free_internal" => {
                    eprintln!();
                    eprintln!(
                        "💡 Did you forget the `esp-alloc` dependency or didn't enable the `compat` feature on it?"
                    );
                    eprintln!();
                }
                _ => (),
            },
            _ => {
                std::process::exit(1);
            }
        }

        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
