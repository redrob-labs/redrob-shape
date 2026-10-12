<!--
SPDX-FileCopyrightText: 2026 Redrob Labs

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Rust third-party dependency inventory

This inventory and the checked-in [`THIRD_PARTY-LICENSES.txt`](THIRD_PARTY-LICENSES.txt) bundle are generated from `Cargo.lock` with `cargo metadata --locked`. Regenerate and verify both with `tools/generate_third_party_licenses.py` whenever locked dependencies change.

### `(MIT OR Apache-2.0) AND Unicode-3.0`

unicode-ident 1.0.24

### `Apache-2.0`

sync_wrapper 1.0.2

### `Apache-2.0 AND ISC`

ring 0.17.14

### `Apache-2.0 OR BSL-1.0`

ryu 1.0.23

### `Apache-2.0 OR ISC OR MIT`

hyper-rustls 0.27.9, rustls 0.23.44

### `Apache-2.0 OR MIT`

atomic-waker 1.1.2, idna_adapter 1.2.2, pin-project-lite 0.2.17, rustc-hash 2.1.3, utf8_iter 1.0.4, zeroize 1.9.0, zeroize_derive 1.5.0

### `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`

wasi 0.11.1+wasi-snapshot-preview1

### `Apache-2.0/MIT`

dbus 0.9.12, libdbus-sys 0.2.7

### `BSD-3-Clause`

subtle 2.6.1

### `CDLA-Permissive-2.0`

webpki-roots 1.0.9

### `ISC`

rustls-webpki 0.103.15, untrusted 0.9.0

### `MIT`

bytes 1.12.1, cfg_aliases 0.2.2, http-body 1.1.0, http-body-util 0.1.5, hyper 1.11.1, hyper-util 0.1.20, mio 1.2.3, slab 0.4.12, synstructure 0.13.2, tokio 1.53.1, tokio-macros 2.7.2, tokio-util 0.7.19, tower 0.5.3, tower-http 0.6.11, tower-layer 0.3.3, tower-service 0.3.3, tracing 0.1.44, tracing-core 0.1.36, try-lock 0.2.5, want 0.3.1, zmij 1.0.23

### `MIT OR Apache-2.0`

base64 0.22.1, bitflags 2.13.1, bumpalo 3.20.3, cc 1.4.5, cfg-if 1.0.4, chacha20 0.10.2, core-foundation 0.10.1, core-foundation 0.9.4, core-foundation-sys 0.8.7, cpufeatures 0.3.1, dbus-secret-service 4.1.0, displaydoc 0.2.7, find-msvc-tools 0.1.12, form_urlencoded 1.2.2, futures-channel 0.3.34, futures-core 0.3.34, futures-macro 0.3.34, futures-sink 0.3.34, futures-task 0.3.34, futures-util 0.3.34, getrandom 0.2.17, getrandom 0.4.3, http 1.5.0, httparse 1.10.1, idna 1.1.0, ipnet 2.12.2, itoa 1.0.18, js-sys 0.3.105, keyring 3.6.3, libc 0.2.189, log 0.4.34, once_cell 1.21.4, percent-encoding 2.3.2, pkg-config 0.3.34, proc-macro2 1.0.107, quinn 0.11.11, quinn-proto 0.11.17, quinn-udp 0.5.15, quote 1.0.47, rand 0.10.2, rand_core 0.10.1, rand_pcg 0.10.2, reqwest 0.12.28, rustls-pki-types 1.15.1, rustversion 1.0.23, security-framework 2.11.1, security-framework 3.7.0, security-framework-sys 2.17.0, serde 1.0.229, serde_core 1.0.229, serde_derive 1.0.229, serde_json 1.0.151, shlex 2.0.1, smallvec 1.16.0, socket2 0.6.5, stable_deref_trait 1.2.1, syn 2.0.119, syn 3.0.5, thiserror 2.0.20, thiserror-impl 2.0.20, tokio-rustls 0.26.5, url 2.5.8, wasm-bindgen 0.2.128, wasm-bindgen-futures 0.4.78, wasm-bindgen-macro 0.2.128, wasm-bindgen-macro-support 0.2.128, wasm-bindgen-shared 0.2.128, web-sys 0.3.105, web-time 1.1.0, windows-link 0.2.1, windows-sys 0.52.0, windows-sys 0.60.2, windows-sys 0.61.2, windows-targets 0.52.6, windows-targets 0.53.5, windows_aarch64_gnullvm 0.52.6, windows_aarch64_gnullvm 0.53.1, windows_aarch64_msvc 0.52.6, windows_aarch64_msvc 0.53.1, windows_i686_gnu 0.52.6, windows_i686_gnu 0.53.1, windows_i686_gnullvm 0.52.6, windows_i686_gnullvm 0.53.1, windows_i686_msvc 0.52.6, windows_i686_msvc 0.53.1, windows_x86_64_gnu 0.52.6, windows_x86_64_gnu 0.53.1, windows_x86_64_gnullvm 0.52.6, windows_x86_64_gnullvm 0.53.1, windows_x86_64_msvc 0.52.6, windows_x86_64_msvc 0.53.1

### `MIT OR Apache-2.0 OR LGPL-2.1-or-later`

r-efi 6.0.0

### `MIT OR Apache-2.0 OR Zlib`

lru-slab 0.1.2, tinyvec_macros 0.1.1

### `MIT/Apache-2.0`

serde_urlencoded 0.7.1

### `Unicode-3.0`

icu_collections 2.3.0, icu_locale_core 2.3.0, icu_normalizer 2.3.0, icu_normalizer_data 2.3.0, icu_properties 2.3.0, icu_properties_data 2.3.0, icu_provider 2.3.1, litemap 0.8.3, potential_utf 0.1.6, tinystr 0.8.4, writeable 0.6.4, yoke 0.8.3, yoke-derive 0.8.2, zerofrom 0.1.8, zerofrom-derive 0.1.7, zerotrie 0.2.5, zerovec 0.11.8, zerovec-derive 0.11.6

### `Unlicense OR MIT`

byteorder 1.5.0, memchr 2.8.3

### `Zlib OR Apache-2.0 OR MIT`

tinyvec 1.13.2
