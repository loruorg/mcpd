# Third-party software

<!-- GENERATED FILE. DO NOT EDIT. Written by tools/third-party.mjs from
     Cargo.lock and web/package-lock.json; run `make third-party`. -->

What mcpd ships that other people wrote, and under which licence. Two
dependency trees reach a release:

- **The Rust crate graph**, linked into the `mcpd` binary. `deny.toml` names
  the licences accepted there and CI enforces it with cargo-deny.
- **The npm tree in `web/`**, which builds the interface. The build output is
  static files, and `include_dir!` compiles those into the same binary. Some
  of these packages, such as React, end up bundled in that output; most are
  build tools that never leave the build machine. Both kinds are listed,
  because both reach the build.

Each list is written from the lock file, so it names the exact versions a
release is built from. Dev-dependencies of the Rust workspace are left out:
a test harness is not distributed.

## The copyleft packages in the web tree

Two families in the npm tree carry a copyleft licence, and neither is in the
`mcpd` binary or in anything it serves.

- **sharp** (`@img/sharp-libvips-*`, LGPL-3.0-or-later, and the `@img/sharp-*`
  platform packages whose expression combines Apache-2.0 with it). Astro
  depends on sharp to process images while the site is built.
- **lightningcss** (`lightningcss` and `lightningcss-*`, MPL-2.0). Tailwind
  depends on it to compile and minify CSS while the site is built.

Both are build-time tools. They run on the build machine, over this
project's own pages and stylesheets, and what they produce is this project's
own content. Neither library is linked into the binary, copied into the
output, or executed by the daemon. The LGPL and the MPL attach conditions to
distributing the licensed work, or a work that incorporates it; running the
work as a tool over your own files does not make the files it produces a
derivative of it. The project's determination is therefore that the
obligations of both licences do not extend to mcpd's release artefacts.
That is the project's reading, recorded here so the question has an answer
on file; it is not legal advice.

## Rust crates (346 crates, 28 licence expressions)

Reachable from a workspace member through a normal or build dependency,
with every Cargo feature on, on every target. A crate that is only used on
one platform is still listed.

### (MIT OR Apache-2.0) AND Unicode-3.0 (1)

- unicode-ident 1.0.24 (https://github.com/dtolnay/unicode-ident)

### 0BSD (1)

- quoted_printable 0.5.2 (https://github.com/staktrace/quoted-printable)

### 0BSD OR MIT OR Apache-2.0 (1)

- adler2 2.0.1 (https://github.com/oyvindln/adler2)

### Apache-2.0 (3)

- flagset 0.4.7 (https://github.com/enarx/flagset)
- sync_wrapper 1.0.2 (https://github.com/Actyx/sync_wrapper)
- unicode-general-category 1.1.0 (https://github.com/yeslogic/unicode-general-category)

### Apache-2.0 AND ISC (1)

- ring 0.17.14 (https://github.com/briansmith/ring)

### Apache-2.0 OR BSL-1.0 (1)

- ryu 1.0.23 (https://github.com/dtolnay/ryu)

### Apache-2.0 OR BSL-1.0 OR MIT (2)

- wasite 1.0.2 (https://github.com/ardaku/wasite)
- whoami 2.1.3 (https://github.com/ardaku/whoami)

### Apache-2.0 OR ISC OR MIT (3)

- hyper-rustls 0.27.9 (https://github.com/rustls/hyper-rustls)
- rustls 0.23.45 (https://github.com/rustls/rustls)
- rustls-native-certs 0.8.4 (https://github.com/rustls/rustls-native-certs)

### Apache-2.0 OR MIT (27)

- atomic-waker 1.1.2 (https://github.com/smol-rs/atomic-waker)
- autocfg 1.5.1 (https://github.com/cuviper/autocfg)
- base64ct 1.8.3 (https://github.com/RustCrypto/formats)
- bit-set 0.8.0 (https://github.com/contain-rs/bit-set)
- bit-vec 0.8.0 (https://github.com/contain-rs/bit-vec)
- bytecount 0.6.9 (https://github.com/llogiq/bytecount)
- cmov 0.5.4 (https://github.com/RustCrypto/utils)
- const-oid 0.10.2 (https://github.com/RustCrypto/formats)
- const-oid 0.9.6 (https://github.com/RustCrypto/formats/tree/master/const-oid)
- ctutils 0.4.2 (https://github.com/RustCrypto/utils)
- der 0.7.10 (https://github.com/RustCrypto/formats/tree/master/der)
- der_derive 0.7.3 (https://github.com/RustCrypto/formats/tree/master/der/derive)
- equivalent 1.0.2 (https://github.com/indexmap-rs/equivalent)
- fastrand 2.5.0 (https://github.com/smol-rs/fastrand)
- idna_adapter 1.2.2 (https://github.com/hsivonen/idna_adapter)
- pin-project-lite 0.2.17 (https://github.com/taiki-e/pin-project-lite)
- rustc-hash 2.1.3 (https://github.com/rust-lang/rustc-hash)
- signature 2.2.0 (https://github.com/RustCrypto/traits/tree/master/signature)
- simd_cesu8 1.2.0 (https://github.com/seancroach/simd_cesu8)
- spki 0.7.3 (https://github.com/RustCrypto/formats/tree/master/spki)
- tls_codec 0.4.2 (https://github.com/RustCrypto/formats)
- tls_codec_derive 0.4.2 (https://github.com/RustCrypto/formats)
- utf8_iter 1.0.4 (https://github.com/hsivonen/utf8_iter)
- utf8parse 0.2.2 (https://github.com/alacritty/vte)
- x509-cert 0.2.5 (https://github.com/RustCrypto/formats/tree/master/x509-cert)
- zeroize 1.9.0 (https://github.com/RustCrypto/utils)
- zeroize_derive 1.5.0 (https://github.com/RustCrypto/utils)

### Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT (4)

- wasi 0.11.1+wasi-snapshot-preview1 (https://github.com/bytecodealliance/wasi)
- wasi 0.14.7+wasi-0.2.4 (https://github.com/bytecodealliance/wasi-rs)
- wasip2 1.0.4+wasi-0.2.12 (https://github.com/bytecodealliance/wasi-rs)
- wit-bindgen 0.57.1 (https://github.com/bytecodealliance/wit-bindgen)

### BSD-2-Clause OR Apache-2.0 OR MIT (2)

- zerocopy 0.8.57 (https://github.com/google/zerocopy)
- zerocopy-derive 0.8.57 (https://github.com/google/zerocopy)

### BSD-3-Clause (1)

- subtle 2.6.1 (https://github.com/dalek-cryptography/subtle)

### CC0-1.0 OR MIT-0 OR Apache-2.0 (1)

- dunce 1.0.5 (https://gitlab.com/kornelski/dunce)

### CDLA-Permissive-2.0 (2)

- webpki-root-certs 1.0.9 (https://github.com/rustls/webpki-roots)
- webpki-roots 1.0.9 (https://github.com/rustls/webpki-roots)

### ISC (3)

- rustls-webpki 0.103.15 (https://github.com/rustls/webpki)
- untrusted 0.7.1 (https://github.com/briansmith/untrusted)
- untrusted 0.9.0 (https://github.com/briansmith/untrusted)

### ISC AND (Apache-2.0 OR ISC) (1)

- aws-lc-rs 1.18.1 (https://github.com/aws/aws-lc-rs)

### ISC AND (Apache-2.0 OR ISC) AND Apache-2.0 AND MIT AND BSD-3-Clause AND (Apache-2.0 OR ISC OR MIT) AND (Apache-2.0 OR ISC OR MIT-0) (1)

- aws-lc-sys 0.45.0 (https://github.com/aws/aws-lc-rs)

### MIT (68)

- axum 0.8.9 (https://github.com/tokio-rs/axum)
- axum-core 0.5.6 (https://github.com/tokio-rs/axum)
- axum-macros 0.5.1 (https://github.com/tokio-rs/axum)
- bytes 1.12.1 (https://github.com/tokio-rs/bytes)
- cfg_aliases 0.2.2 (https://github.com/katharostech/cfg_aliases)
- combine 4.6.8 (https://github.com/Marwes/combine)
- data-encoding 2.11.1 (https://github.com/ia0/data-encoding)
- email_address 0.2.9 (https://github.com/johnstonskj/rust-email_address.git)
- fancy-regex 0.19.1 (https://github.com/fancy-regex/fancy-regex)
- fluent-uri 0.4.1 (https://github.com/yescallop/fluent-uri-rs)
- fs_extra 1.3.0 (https://github.com/webdesus/fs_extra)
- generic-array 0.14.7 (https://github.com/fizyk20/generic-array.git)
- hostname 0.4.2 (https://github.com/djc/hostname)
- http-body 1.1.0 (https://github.com/hyperium/http-body)
- http-body-util 0.1.5 (https://github.com/hyperium/http-body)
- hyper 1.11.1 (https://github.com/hyperium/hyper)
- hyper-util 0.1.20 (https://github.com/hyperium/hyper-util)
- include_dir 0.7.4 (https://github.com/Michael-F-Bryan/include_dir)
- include_dir_macros 0.7.4 (https://github.com/Michael-F-Bryan/include_dir)
- jsonschema 0.56.0 (https://github.com/Stranger6667/jsonschema)
- jsonschema-regex 0.56.0 (https://github.com/Stranger6667/jsonschema)
- jsonschema-value 0.56.0 (https://github.com/Stranger6667/jsonschema)
- jsonwebtoken 11.1.0 (https://github.com/Keats/jsonwebtoken)
- lettre 0.11.23 (https://github.com/lettre/lettre)
- libredox 0.1.24 (https://gitlab.redox-os.org/redox-os/libredox.git)
- libsqlite3-sys 0.38.2 (https://github.com/rusqlite/rusqlite)
- matchers 0.2.0 (https://github.com/hawkw/matchers)
- micromap 0.3.0 (https://github.com/yegor256/micromap)
- mio 1.2.3 (https://github.com/tokio-rs/mio)
- nom 8.0.0 (https://github.com/rust-bakery/nom)
- nu-ansi-term 0.50.3 (https://github.com/nushell/nu-ansi-term)
- outref 0.5.2 (https://github.com/Nugine/outref)
- phf 0.13.1 (https://github.com/rust-phf/rust-phf)
- phf_shared 0.13.1 (https://github.com/rust-phf/rust-phf)
- redox_syscall 0.5.18 (https://gitlab.redox-os.org/redox-os/syscall)
- referencing 0.56.0 (https://github.com/Stranger6667/jsonschema)
- rsqlite-vfs 0.1.1
- rusqlite 0.40.2 (https://github.com/rusqlite/rusqlite)
- schannel 0.1.29 (https://github.com/steffengy/schannel-rs)
- sharded-slab 0.1.7 (https://github.com/hawkw/sharded-slab)
- simd-adler32 0.3.10 (https://github.com/mcountryman/simd-adler32)
- slab 0.4.12 (https://github.com/tokio-rs/slab)
- sqlite-wasm-rs 0.5.5 (https://github.com/Spxg/sqlite-wasm-rs)
- strsim 0.11.1 (https://github.com/rapidfuzz/strsim-rs)
- strum 0.28.0 (https://github.com/Peternator7/strum)
- strum_macros 0.28.0 (https://github.com/Peternator7/strum)
- synstructure 0.13.2 (https://github.com/mystor/synstructure)
- tokio 1.53.1 (https://github.com/tokio-rs/tokio)
- tokio-macros 2.7.2 (https://github.com/tokio-rs/tokio)
- tokio-postgres-rustls 0.14.0 (https://github.com/jbg/tokio-postgres-rustls)
- tokio-util 0.7.19 (https://github.com/tokio-rs/tokio)
- tower 0.5.3 (https://github.com/tower-rs/tower)
- tower-http 0.6.11 (https://github.com/tower-rs/tower-http)
- tower-http 0.7.1 (https://github.com/tower-rs/tower-http)
- tower-layer 0.3.3 (https://github.com/tower-rs/tower)
- tower-service 0.3.3 (https://github.com/tower-rs/tower)
- tracing 0.1.44 (https://github.com/tokio-rs/tracing)
- tracing-attributes 0.1.31 (https://github.com/tokio-rs/tracing)
- tracing-core 0.1.36 (https://github.com/tokio-rs/tracing)
- tracing-log 0.2.0 (https://github.com/tokio-rs/tracing)
- tracing-serde 0.2.0 (https://github.com/tokio-rs/tracing)
- tracing-subscriber 0.3.23 (https://github.com/tokio-rs/tracing)
- try-lock 0.2.5 (https://github.com/seanmonstar/try-lock)
- uuid-simd 0.8.0 (https://github.com/Nugine/simd)
- valuable 0.1.1 (https://github.com/tokio-rs/valuable)
- vsimd 0.8.0 (https://github.com/Nugine/simd)
- want 0.3.1 (https://github.com/seanmonstar/want)
- zmij 1.0.23 (https://github.com/dtolnay/zmij)

### MIT AND BSD-3-Clause (1)

- matchit 0.8.4 (https://github.com/ibraheemdev/matchit)

### MIT OR Apache-2.0 (187)

- ahash 0.8.12 (https://github.com/tkaitchuck/ahash)
- allocator-api2 0.2.21 (https://github.com/zakarumych/allocator-api2)
- anstream 1.0.0 (https://github.com/rust-cli/anstyle.git)
- anstyle 1.0.14 (https://github.com/rust-cli/anstyle.git)
- anstyle-parse 1.0.0 (https://github.com/rust-cli/anstyle.git)
- anstyle-query 1.1.5 (https://github.com/rust-cli/anstyle.git)
- anstyle-wincon 3.0.11 (https://github.com/rust-cli/anstyle.git)
- anyhow 1.0.104 (https://github.com/dtolnay/anyhow)
- argon2 0.5.3 (https://github.com/RustCrypto/password-hashes/tree/master/argon2)
- async-compression 0.4.46 (https://github.com/Nullus157/async-compression)
- async-trait 0.1.92 (https://github.com/dtolnay/async-trait)
- base64 0.22.1 (https://github.com/marshallpierce/rust-base64)
- base64 0.23.1 (https://github.com/marshallpierce/rust-base64)
- bitflags 2.13.2 (https://github.com/bitflags/bitflags)
- blake2 0.10.6 (https://github.com/RustCrypto/hashes)
- block-buffer 0.10.4 (https://github.com/RustCrypto/utils)
- block-buffer 0.12.1 (https://github.com/RustCrypto/utils)
- bumpalo 3.20.3 (https://github.com/fitzgen/bumpalo)
- cc 1.4.5 (https://github.com/rust-lang/cc-rs)
- cfg-if 1.0.4 (https://github.com/rust-lang/cfg-if)
- chacha20 0.10.2 (https://github.com/RustCrypto/stream-ciphers)
- clap 4.6.6 (https://github.com/clap-rs/clap)
- clap_builder 4.6.6 (https://github.com/clap-rs/clap)
- clap_derive 4.6.4 (https://github.com/clap-rs/clap)
- clap_lex 1.1.0 (https://github.com/clap-rs/clap)
- cmake 0.1.58 (https://github.com/rust-lang/cmake-rs)
- colorchoice 1.0.5 (https://github.com/rust-cli/anstyle.git)
- compression-codecs 0.4.41 (https://github.com/Nullus157/async-compression)
- compression-core 0.4.33 (https://github.com/Nullus157/async-compression)
- core-foundation 0.10.1 (https://github.com/servo/core-foundation-rs)
- core-foundation-sys 0.8.7 (https://github.com/servo/core-foundation-rs)
- cpufeatures 0.2.17 (https://github.com/RustCrypto/utils)
- cpufeatures 0.3.1 (https://github.com/RustCrypto/utils)
- crc32fast 1.5.2 (https://github.com/srijs/rust-crc32fast)
- crypto-common 0.1.7 (https://github.com/RustCrypto/traits)
- crypto-common 0.2.2 (https://github.com/RustCrypto/traits)
- deadpool 0.13.1 (https://github.com/deadpool-rs/deadpool)
- deadpool-postgres 0.14.2 (https://github.com/deadpool-rs/deadpool)
- deadpool-runtime 0.3.1 (https://github.com/deadpool-rs/deadpool)
- deranged 0.5.8 (https://github.com/jhpratt/deranged)
- digest 0.10.7 (https://github.com/RustCrypto/traits)
- digest 0.11.3 (https://github.com/RustCrypto/traits)
- displaydoc 0.2.7 (https://github.com/yaahc/displaydoc)
- email-encoding 0.4.2 (https://github.com/lettre/email-encoding)
- errno 0.3.14 (https://github.com/lambda-fairy/rust-errno)
- fallible-iterator 0.2.0 (https://github.com/sfackler/rust-fallible-iterator)
- fallible-iterator 0.3.0 (https://github.com/sfackler/rust-fallible-iterator)
- fallible-streaming-iterator 0.1.9 (https://github.com/sfackler/fallible-streaming-iterator)
- find-msvc-tools 0.1.12 (https://github.com/rust-lang/cc-rs)
- flate2 1.1.10 (https://github.com/rust-lang/flate2-rs)
- form_urlencoded 1.2.2 (https://github.com/servo/rust-url)
- fraction 0.17.0 (https://github.com/dnsl48/fraction.git)
- futures 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-channel 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-core 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-executor 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-io 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-macro 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-sink 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-task 0.3.34 (https://github.com/rust-lang/futures-rs)
- futures-util 0.3.34 (https://github.com/rust-lang/futures-rs)
- getrandom 0.2.17 (https://github.com/rust-random/getrandom)
- getrandom 0.3.4 (https://github.com/rust-random/getrandom)
- getrandom 0.4.3 (https://github.com/rust-random/getrandom)
- hashbrown 0.16.1 (https://github.com/rust-lang/hashbrown)
- hashbrown 0.17.1 (https://github.com/rust-lang/hashbrown)
- hashlink 0.12.2 (https://github.com/djc/hashlink)
- heck 0.5.0 (https://github.com/withoutboats/heck)
- hex 0.4.3 (https://github.com/KokaKiwi/rust-hex)
- hmac 0.13.0 (https://github.com/RustCrypto/MACs)
- http 1.5.0 (https://github.com/hyperium/http)
- httparse 1.10.1 (https://github.com/seanmonstar/httparse)
- httpdate 1.0.3 (https://github.com/pyfisch/httpdate)
- hybrid-array 0.4.15 (https://github.com/RustCrypto/hybrid-array)
- idna 1.1.0 (https://github.com/servo/rust-url/)
- ipnet 2.12.2 (https://github.com/krisprice/ipnet)
- is_terminal_polyfill 1.70.2 (https://github.com/polyfill-rs/is_terminal_polyfill)
- itoa 1.0.18 (https://github.com/dtolnay/itoa)
- jni 0.22.4 (https://github.com/jni-rs/jni-rs)
- jni-macros 0.22.4 (https://github.com/jni-rs/jni-rs)
- jni-sys 0.4.1 (https://github.com/jni-rs/jni-sys)
- jni-sys-macros 0.4.1 (https://github.com/jni-rs/jni-sys)
- jobserver 0.1.35 (https://github.com/rust-lang/jobserver-rs)
- js-sys 0.3.105 (https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/js-sys)
- lazy_static 1.5.0 (https://github.com/rust-lang-nursery/lazy-static.rs)
- libc 0.2.189 (https://github.com/rust-lang/libc)
- lock_api 0.4.14 (https://github.com/Amanieu/parking_lot)
- log 0.4.34 (https://github.com/rust-lang/log)
- md-5 0.11.0 (https://github.com/RustCrypto/hashes)
- mime 0.3.17 (https://github.com/hyperium/mime)
- num 0.4.3 (https://github.com/rust-num/num)
- num-bigint 0.4.8 (https://github.com/rust-num/num-bigint)
- num-cmp 0.1.0 (https://github.com/lifthrasiir/num-cmp)
- num-complex 0.4.6 (https://github.com/rust-num/num-complex)
- num-conv 0.2.2 (https://github.com/jhpratt/num-conv)
- num-integer 0.1.47 (https://github.com/rust-num/num-integer)
- num-iter 0.1.46 (https://github.com/rust-num/num-iter)
- num-rational 0.4.2 (https://github.com/rust-num/num-rational)
- num-traits 0.2.19 (https://github.com/rust-num/num-traits)
- once_cell 1.21.4 (https://github.com/matklad/once_cell)
- once_cell_polyfill 1.70.2 (https://github.com/polyfill-rs/once_cell_polyfill)
- openssl-probe 0.2.1 (https://github.com/rustls/openssl-probe)
- parking_lot 0.12.5 (https://github.com/Amanieu/parking_lot)
- parking_lot_core 0.9.12 (https://github.com/Amanieu/parking_lot)
- password-hash 0.5.0 (https://github.com/RustCrypto/traits/tree/master/password-hash)
- percent-encoding 2.3.2 (https://github.com/servo/rust-url/)
- pkg-config 0.3.34 (https://github.com/rust-lang/pkg-config-rs)
- postgres-protocol 0.6.12 (https://github.com/rust-postgres/rust-postgres)
- postgres-types 0.2.14 (https://github.com/rust-postgres/rust-postgres)
- powerfmt 0.2.0 (https://github.com/jhpratt/powerfmt)
- proc-macro2 1.0.107 (https://github.com/dtolnay/proc-macro2)
- qrcode 0.14.1 (https://github.com/kennytm/qrcode-rust)
- quinn 0.11.11 (https://github.com/quinn-rs/quinn)
- quinn-proto 0.11.17 (https://github.com/quinn-rs/quinn)
- quinn-udp 0.5.15 (https://github.com/quinn-rs/quinn)
- quote 1.0.47 (https://github.com/dtolnay/quote)
- rand 0.10.2 (https://github.com/rust-random/rand)
- rand_core 0.10.1 (https://github.com/rust-random/rand_core)
- rand_core 0.6.4 (https://github.com/rust-random/rand)
- rand_pcg 0.10.2 (https://github.com/rust-random/rngs)
- ref-cast 1.0.27 (https://github.com/dtolnay/ref-cast)
- ref-cast-impl 1.0.27 (https://github.com/dtolnay/ref-cast)
- regex 1.13.1 (https://github.com/rust-lang/regex)
- regex-automata 0.4.18 (https://github.com/rust-lang/regex)
- regex-syntax 0.8.11 (https://github.com/rust-lang/regex)
- reqwest 0.13.5 (https://github.com/seanmonstar/reqwest)
- rustc_version 0.4.1 (https://github.com/djc/rustc-version-rs)
- rustls-pki-types 1.15.1 (https://github.com/rustls/pki-types)
- rustls-platform-verifier 0.7.0 (https://github.com/rustls/rustls-platform-verifier)
- rustls-platform-verifier-android 0.1.1 (https://github.com/rustls/rustls-platform-verifier)
- rustversion 1.0.23 (https://github.com/dtolnay/rustversion)
- scopeguard 1.2.0 (https://github.com/bluss/scopeguard)
- security-framework 3.7.0 (https://github.com/kornelski/rust-security-framework)
- security-framework-sys 2.17.0 (https://github.com/kornelski/rust-security-framework)
- semver 1.0.28 (https://github.com/dtolnay/semver)
- serde 1.0.229 (https://github.com/serde-rs/serde)
- serde_core 1.0.229 (https://github.com/serde-rs/serde)
- serde_derive 1.0.229 (https://github.com/serde-rs/serde)
- serde_json 1.0.151 (https://github.com/serde-rs/json)
- serde_path_to_error 0.1.20 (https://github.com/dtolnay/path-to-error)
- serde_urlencoded 0.7.1 (https://github.com/nox/serde_urlencoded)
- sha2 0.10.9 (https://github.com/RustCrypto/hashes)
- sha2 0.11.0 (https://github.com/RustCrypto/hashes)
- shlex 2.0.1 (https://github.com/comex/rust-shlex)
- signal-hook-registry 1.4.8 (https://github.com/vorner/signal-hook)
- simdutf8 0.1.5 (https://github.com/rusticstuff/simdutf8)
- siphasher 1.0.3 (https://github.com/jedisct1/rust-siphash)
- smallvec 1.16.1 (https://github.com/servo/rust-smallvec)
- socket2 0.6.5 (https://github.com/rust-lang/socket2)
- stable_deref_trait 1.2.1 (https://github.com/storyyeller/stable_deref_trait)
- stringprep 0.1.5 (https://github.com/sfackler/rust-stringprep)
- syn 2.0.119 (https://github.com/dtolnay/syn)
- syn 3.0.5 (https://github.com/dtolnay/syn)
- thiserror 2.0.20 (https://github.com/dtolnay/thiserror)
- thiserror-impl 2.0.20 (https://github.com/dtolnay/thiserror)
- thread_local 1.1.10 (https://github.com/Amanieu/thread_local-rs)
- time 0.3.55 (https://github.com/time-rs/time)
- time-core 0.1.9 (https://github.com/time-rs/time)
- time-macros 0.2.32 (https://github.com/time-rs/time)
- tokio-postgres 0.7.18 (https://github.com/rust-postgres/rust-postgres)
- tokio-rustls 0.26.5 (https://github.com/rustls/tokio-rustls)
- typenum 1.20.1 (https://github.com/paholg/typenum)
- unicode-bidi 0.3.18 (https://github.com/servo/unicode-bidi)
- unicode-normalization 0.1.25 (https://github.com/unicode-rs/unicode-normalization)
- unicode-properties 0.1.4 (https://github.com/unicode-rs/unicode-properties)
- url 2.5.8 (https://github.com/servo/rust-url)
- vcpkg 0.2.15 (https://github.com/mcgoo/vcpkg-rs)
- version_check 0.9.5 (https://github.com/SergioBenitez/version_check)
- wasm-bindgen 0.2.128 (https://github.com/wasm-bindgen/wasm-bindgen)
- wasm-bindgen-futures 0.4.78 (https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/futures)
- wasm-bindgen-macro 0.2.128 (https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/macro)
- wasm-bindgen-macro-support 0.2.128 (https://github.com/wasm-bindgen/wasm-bindgen/tree/main/crates/macro-support)
- wasm-bindgen-shared 0.2.128 (https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/shared)
- web-sys 0.3.105 (https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/web-sys)
- web-time 1.1.0 (https://github.com/daxpedda/web-time)
- windows_aarch64_gnullvm 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_aarch64_msvc 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_i686_gnu 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_i686_gnullvm 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_i686_msvc 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_x86_64_gnu 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_x86_64_gnullvm 0.52.6 (https://github.com/microsoft/windows-rs)
- windows_x86_64_msvc 0.52.6 (https://github.com/microsoft/windows-rs)
- windows-link 0.2.1 (https://github.com/microsoft/windows-rs)
- windows-sys 0.52.0 (https://github.com/microsoft/windows-rs)
- windows-sys 0.61.2 (https://github.com/microsoft/windows-rs)
- windows-targets 0.52.6 (https://github.com/microsoft/windows-rs)

### MIT OR Apache-2.0 OR LGPL-2.1-or-later (2)

- r-efi 5.3.0 (https://github.com/r-efi/r-efi)
- r-efi 6.0.0 (https://github.com/r-efi/r-efi)

### MIT OR Apache-2.0 OR Zlib (2)

- lru-slab 0.1.2 (https://github.com/Ralith/lru-slab)
- tinyvec_macros 0.1.1 (https://github.com/Soveu/tinyvec_macros)

### MIT OR Zlib OR Apache-2.0 (1)

- miniz_oxide 0.9.1 (https://github.com/Frommi/miniz_oxide/tree/master/miniz_oxide)

### MIT-0 (1)

- borrow-or-share 0.2.4 (https://github.com/yescallop/borrow-or-share)

### Unicode-3.0 (18)

- icu_collections 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_locale_core 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_normalizer 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_normalizer_data 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_properties 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_properties_data 2.3.0 (https://github.com/unicode-org/icu4x)
- icu_provider 2.3.1 (https://github.com/unicode-org/icu4x)
- litemap 0.8.3 (https://github.com/unicode-org/icu4x)
- potential_utf 0.1.6 (https://github.com/unicode-org/icu4x)
- tinystr 0.8.4 (https://github.com/unicode-org/icu4x)
- writeable 0.6.4 (https://github.com/unicode-org/icu4x)
- yoke 0.8.3 (https://github.com/unicode-org/icu4x)
- yoke-derive 0.8.2 (https://github.com/unicode-org/icu4x)
- zerofrom 0.1.8 (https://github.com/unicode-org/icu4x)
- zerofrom-derive 0.1.7 (https://github.com/unicode-org/icu4x)
- zerotrie 0.2.5 (https://github.com/unicode-org/icu4x)
- zerovec 0.11.8 (https://github.com/unicode-org/icu4x)
- zerovec-derive 0.11.6 (https://github.com/unicode-org/icu4x)

### Unlicense OR MIT (6)

- aho-corasick 1.1.5 (https://github.com/BurntSushi/aho-corasick)
- byteorder 1.5.0 (https://github.com/BurntSushi/byteorder)
- memchr 2.8.3 (https://github.com/BurntSushi/memchr)
- same-file 1.0.6 (https://github.com/BurntSushi/same-file)
- walkdir 2.5.0 (https://github.com/BurntSushi/walkdir)
- winapi-util 0.1.11 (https://github.com/BurntSushi/winapi-util)

### Zlib (2)

- foldhash 0.2.0 (https://github.com/orlp/foldhash)
- zlib-rs 0.6.7 (https://github.com/trifectatechfoundation/zlib-rs)

### Zlib OR Apache-2.0 OR MIT (3)

- objc2-core-foundation 0.3.2 (https://github.com/madsmtm/objc2)
- objc2-system-configuration 0.3.2 (https://github.com/madsmtm/objc2)
- tinyvec 1.13.2 (https://github.com/Lokathor/tinyvec)

## npm packages in web/ (434 packages, 14 licence expressions)

Every entry in `web/package-lock.json`, including the platform-specific
binaries npm marks optional and the packages only the build needs.

### 0BSD (2)

- tslib 2.8.1
- tslib 2.8.1

### Apache-2.0 (19)

- @img/sharp-darwin-arm64 0.35.4
- @img/sharp-darwin-x64 0.35.4
- @img/sharp-freebsd-wasm32 0.35.4
- @img/sharp-linux-arm 0.35.4
- @img/sharp-linux-arm64 0.35.4
- @img/sharp-linux-ppc64 0.35.4
- @img/sharp-linux-riscv64 0.35.4
- @img/sharp-linux-s390x 0.35.4
- @img/sharp-linux-x64 0.35.4
- @img/sharp-linuxmusl-arm64 0.35.4
- @img/sharp-linuxmusl-x64 0.35.4
- @img/sharp-webcontainers-wasm32 0.35.4
- aria-query 5.3.2
- axobject-query 4.1.0
- baseline-browser-mapping 2.11.23
- class-variance-authority 0.7.1
- detect-libc 2.1.2
- sharp 0.35.4
- typescript 6.0.3

### Apache-2.0 AND LGPL-3.0-or-later (3)

- @img/sharp-win32-arm64 0.35.4
- @img/sharp-win32-ia32 0.35.4
- @img/sharp-win32-x64 0.35.4

### Apache-2.0 AND LGPL-3.0-or-later AND MIT (1)

- @img/sharp-wasm32 0.35.4

### BlueOak-1.0.0 (3)

- common-ancestor-path 2.0.0
- lru-cache 11.5.2
- sax 1.6.1

### BSD-2-Clause (8)

- css-select 6.0.0
- css-what 7.0.0
- domelementtype 2.3.0
- domhandler 5.0.3
- domutils 3.2.2
- entities 4.5.0
- http-cache-semantics 4.2.0
- nth-check 2.1.1

### BSD-3-Clause (3)

- diff 9.0.0
- smol-toml 1.8.0
- source-map-js 1.2.1

### CC-BY-4.0 (1)

- caniuse-lite 1.0.30001810

### CC0-1.0 (2)

- mdn-data 2.0.28
- mdn-data 2.27.1

### ISC (14)

- @ungap/structured-clone 1.4.0
- anymatch 3.1.3
- boolbase 1.0.0
- electron-to-chromium 1.5.427
- github-slugger 2.0.0
- graceful-fs 4.2.11
- lru-cache 5.1.1
- piccolore 0.1.3
- picocolors 1.1.1
- semver 6.3.1
- semver 7.8.5
- semver 7.8.5
- yallist 3.1.1
- yargs-parser 22.0.0

### LGPL-3.0-or-later (10)

- @img/sharp-libvips-darwin-arm64 1.3.3
- @img/sharp-libvips-darwin-x64 1.3.3
- @img/sharp-libvips-linux-arm 1.3.3
- @img/sharp-libvips-linux-arm64 1.3.3
- @img/sharp-libvips-linux-ppc64 1.3.3
- @img/sharp-libvips-linux-riscv64 1.3.3
- @img/sharp-libvips-linux-s390x 1.3.3
- @img/sharp-libvips-linux-x64 1.3.3
- @img/sharp-libvips-linuxmusl-arm64 1.3.3
- @img/sharp-libvips-linuxmusl-x64 1.3.3

### MIT (343)

- @astrojs/compiler-binding 0.4.0
- @astrojs/compiler-binding-darwin-arm64 0.4.0
- @astrojs/compiler-binding-darwin-x64 0.4.0
- @astrojs/compiler-binding-linux-arm64-gnu 0.4.0
- @astrojs/compiler-binding-linux-arm64-musl 0.4.0
- @astrojs/compiler-binding-linux-x64-gnu 0.4.0
- @astrojs/compiler-binding-linux-x64-musl 0.4.0
- @astrojs/compiler-binding-wasm32-wasi 0.4.0
- @astrojs/compiler-binding-win32-arm64-msvc 0.4.0
- @astrojs/compiler-binding-win32-x64-msvc 0.4.0
- @astrojs/compiler-rs 0.4.0
- @astrojs/internal-helpers 0.11.0
- @astrojs/markdown-satteri 0.4.1
- @astrojs/prism 4.0.2
- @astrojs/react 6.0.5
- @astrojs/sitemap 3.7.4
- @astrojs/telemetry 3.3.3
- @babel/code-frame 7.29.7
- @babel/compat-data 7.29.7
- @babel/core 7.29.7
- @babel/generator 7.29.8
- @babel/helper-compilation-targets 7.29.7
- @babel/helper-globals 7.29.7
- @babel/helper-module-imports 7.29.7
- @babel/helper-module-transforms 7.29.7
- @babel/helper-plugin-utils 7.29.7
- @babel/helper-string-parser 7.29.7
- @babel/helper-validator-identifier 7.29.7
- @babel/helper-validator-option 7.29.7
- @babel/helpers 7.29.7
- @babel/parser 7.29.8
- @babel/plugin-transform-react-jsx-self 7.29.7
- @babel/plugin-transform-react-jsx-source 7.29.7
- @babel/template 7.29.7
- @babel/traverse 7.29.8
- @babel/types 7.29.8
- @bruits/satteri-darwin-arm64 0.10.5
- @bruits/satteri-darwin-x64 0.10.5
- @bruits/satteri-linux-arm64-gnu 0.10.5
- @bruits/satteri-linux-arm64-musl 0.10.5
- @bruits/satteri-linux-x64-gnu 0.10.5
- @bruits/satteri-linux-x64-musl 0.10.5
- @bruits/satteri-wasm32-wasi 0.10.5
- @bruits/satteri-win32-arm64-msvc 0.10.5
- @bruits/satteri-win32-x64-msvc 0.10.5
- @capsizecss/unpack 4.0.1
- @clack/core 1.5.0
- @clack/prompts 1.8.0
- @emnapi/core 1.11.1
- @emnapi/core 1.11.1
- @emnapi/core 1.11.3
- @emnapi/runtime 1.11.1
- @emnapi/runtime 1.11.1
- @emnapi/runtime 1.11.3
- @emnapi/wasi-threads 1.2.2
- @emnapi/wasi-threads 1.2.2
- @emnapi/wasi-threads 1.2.3
- @esbuild/aix-ppc64 0.28.2
- @esbuild/android-arm 0.28.2
- @esbuild/android-arm64 0.28.2
- @esbuild/android-x64 0.28.2
- @esbuild/darwin-arm64 0.28.2
- @esbuild/darwin-x64 0.28.2
- @esbuild/freebsd-arm64 0.28.2
- @esbuild/freebsd-x64 0.28.2
- @esbuild/linux-arm 0.28.2
- @esbuild/linux-arm64 0.28.2
- @esbuild/linux-ia32 0.28.2
- @esbuild/linux-loong64 0.28.2
- @esbuild/linux-mips64el 0.28.2
- @esbuild/linux-ppc64 0.28.2
- @esbuild/linux-riscv64 0.28.2
- @esbuild/linux-s390x 0.28.2
- @esbuild/linux-x64 0.28.2
- @esbuild/netbsd-arm64 0.28.2
- @esbuild/netbsd-x64 0.28.2
- @esbuild/openbsd-arm64 0.28.2
- @esbuild/openbsd-x64 0.28.2
- @esbuild/openharmony-arm64 0.28.2
- @esbuild/sunos-x64 0.28.2
- @esbuild/win32-arm64 0.28.2
- @esbuild/win32-ia32 0.28.2
- @esbuild/win32-x64 0.28.2
- @floating-ui/core 1.8.0
- @floating-ui/dom 1.8.0
- @floating-ui/react-dom 2.1.9
- @floating-ui/utils 0.2.12
- @img/colour 1.1.0
- @jridgewell/gen-mapping 0.3.13
- @jridgewell/remapping 2.3.5
- @jridgewell/resolve-uri 3.1.2
- @jridgewell/sourcemap-codec 1.6.0
- @jridgewell/trace-mapping 0.3.31
- @napi-rs/wasm-runtime 1.1.4
- @napi-rs/wasm-runtime 1.2.4
- @oslojs/encoding 1.1.0
- @oxc-project/types 0.149.0
- @radix-ui/number 1.1.3
- @radix-ui/primitive 1.1.7
- @radix-ui/react-accordion 1.2.20
- @radix-ui/react-arrow 1.1.15
- @radix-ui/react-collapsible 1.1.20
- @radix-ui/react-collection 1.1.15
- @radix-ui/react-compose-refs 1.1.5
- @radix-ui/react-context 1.2.2
- @radix-ui/react-dialog 1.1.23
- @radix-ui/react-direction 1.1.4
- @radix-ui/react-dismissable-layer 1.1.19
- @radix-ui/react-dropdown-menu 2.1.24
- @radix-ui/react-focus-guards 1.1.6
- @radix-ui/react-focus-scope 1.1.16
- @radix-ui/react-id 1.1.4
- @radix-ui/react-menu 2.1.24
- @radix-ui/react-popper 1.3.7
- @radix-ui/react-portal 1.1.17
- @radix-ui/react-presence 1.1.10
- @radix-ui/react-primitive 2.1.10
- @radix-ui/react-roving-focus 1.1.19
- @radix-ui/react-select 2.3.7
- @radix-ui/react-separator 1.1.15
- @radix-ui/react-slot 1.3.3
- @radix-ui/react-tabs 1.1.21
- @radix-ui/react-tooltip 1.2.16
- @radix-ui/react-use-callback-ref 1.1.4
- @radix-ui/react-use-controllable-state 1.2.6
- @radix-ui/react-use-effect-event 0.0.5
- @radix-ui/react-use-is-hydrated 0.1.3
- @radix-ui/react-use-layout-effect 1.1.4
- @radix-ui/react-use-previous 1.1.4
- @radix-ui/react-use-rect 1.1.4
- @radix-ui/react-use-size 1.1.4
- @radix-ui/react-visually-hidden 1.2.11
- @radix-ui/rect 1.1.3
- @rolldown/binding-android-arm-eabi 1.2.8
- @rolldown/binding-android-arm64 1.2.8
- @rolldown/binding-darwin-arm64 1.2.8
- @rolldown/binding-darwin-x64 1.2.8
- @rolldown/binding-freebsd-x64 1.2.8
- @rolldown/binding-linux-arm-gnueabihf 1.2.8
- @rolldown/binding-linux-arm64-gnu 1.2.8
- @rolldown/binding-linux-arm64-musl 1.2.8
- @rolldown/binding-linux-ppc64-gnu 1.2.8
- @rolldown/binding-linux-s390x-gnu 1.2.8
- @rolldown/binding-linux-x64-gnu 1.2.8
- @rolldown/binding-linux-x64-musl 1.2.8
- @rolldown/binding-openharmony-arm64 1.2.8
- @rolldown/binding-win32-arm64-msvc 1.2.8
- @rolldown/binding-win32-x64-msvc 1.2.8
- @rolldown/pluginutils 1.0.0-rc.3
- @rolldown/pluginutils 1.0.1
- @shikijs/core 4.4.3
- @shikijs/engine-javascript 4.4.3
- @shikijs/engine-oniguruma 4.4.3
- @shikijs/langs 4.4.3
- @shikijs/primitive 4.4.3
- @shikijs/themes 4.4.3
- @shikijs/types 4.4.3
- @shikijs/vscode-textmate 10.0.2
- @tailwindcss/node 4.3.3
- @tailwindcss/oxide 4.3.3
- @tailwindcss/oxide-android-arm64 4.3.3
- @tailwindcss/oxide-darwin-arm64 4.3.3
- @tailwindcss/oxide-darwin-x64 4.3.3
- @tailwindcss/oxide-freebsd-x64 4.3.3
- @tailwindcss/oxide-linux-arm-gnueabihf 4.3.3
- @tailwindcss/oxide-linux-arm64-gnu 4.3.3
- @tailwindcss/oxide-linux-arm64-musl 4.3.3
- @tailwindcss/oxide-linux-x64-gnu 4.3.3
- @tailwindcss/oxide-linux-x64-musl 4.3.3
- @tailwindcss/oxide-wasm32-wasi 4.3.3
- @tailwindcss/oxide-win32-arm64-msvc 4.3.3
- @tailwindcss/oxide-win32-x64-msvc 4.3.3
- @tailwindcss/vite 4.3.3
- @tybys/wasm-util 0.10.2
- @tybys/wasm-util 0.10.3
- @types/babel__core 7.20.5
- @types/babel__generator 7.27.0
- @types/babel__template 7.4.4
- @types/babel__traverse 7.28.0
- @types/estree 1.0.9
- @types/estree-jsx 1.0.5
- @types/hast 3.0.5
- @types/mdast 4.0.4
- @types/nlcst 2.0.3
- @types/node 24.13.3
- @types/node 26.2.0
- @types/react 19.3.0
- @types/react-dom 19.3.0
- @types/sax 1.2.7
- @types/unist 3.0.3
- @vitejs/plugin-react 5.2.0
- am-i-vibing 0.4.0
- arg 5.0.2
- aria-hidden 1.2.6
- astro 7.3.2
- bail 2.0.2
- browserslist 4.28.9
- ccount 2.0.1
- character-entities-html4 2.1.0
- character-entities-legacy 3.0.0
- chokidar 5.0.0
- ci-info 4.4.0
- clsx 2.1.1
- comma-separated-tokens 2.0.3
- commander 11.1.0
- convert-source-map 2.0.0
- cookie 2.0.1
- cookie-es 1.2.3
- crossws 0.3.5
- css-tree 2.2.1
- css-tree 3.2.1
- csso 5.0.5
- csstype 3.2.3
- debug 4.4.3
- defu 6.1.7
- dequal 2.0.3
- destr 2.0.5
- detect-node-es 1.1.0
- devalue 5.9.4
- devlop 1.1.0
- dom-serializer 2.0.0
- dset 3.1.4
- enhanced-resolve 5.24.5
- es-module-lexer 2.3.2
- esbuild 0.28.2
- escalade 3.2.0
- eventemitter3 5.0.4
- extend 3.0.2
- fast-string-truncated-width 3.0.3
- fast-string-width 3.0.2
- fast-wrap-ansi 0.2.2
- fdir 6.5.0
- find-proc 0.1.0
- flattie 1.1.1
- fontace 0.4.1
- fontkitten 1.0.3
- fsevents 2.3.3
- gensync 1.0.0-beta.2
- get-nonce 1.0.1
- get-tsconfig 5.0.0-beta.4
- h3 1.15.11
- hast-util-to-html 9.0.5
- hast-util-whitespace 3.0.0
- html-escaper 3.0.3
- html-void-elements 3.0.0
- iron-webcrypto 1.2.1
- is-docker 4.0.0
- is-plain-obj 4.1.0
- jiti 2.7.0
- js-tokens 4.0.0
- js-yaml 4.3.2
- jsesc 3.1.0
- json5 2.2.3
- jsonc-parser 3.3.1
- magic-string 0.30.21
- magic-string 1.3.1
- magicast 0.5.4
- mdast-util-to-hast 13.2.1
- micromark-util-character 2.1.1
- micromark-util-encode 2.0.1
- micromark-util-sanitize-uri 2.0.1
- micromark-util-symbol 2.0.1
- micromark-util-types 2.0.2
- mrmime 2.0.1
- ms 2.1.3
- nanoid 3.3.19
- neotraverse 1.0.1
- nlcst-to-string 4.0.0
- node-fetch-native 1.6.7
- node-mock-http 1.0.5
- node-releases 2.0.55
- normalize-path 3.0.0
- obug 2.2.1
- ofetch 1.5.1
- ohash 2.0.12
- oniguruma-parser 0.12.2
- oniguruma-to-es 4.3.6
- p-limit 7.3.2
- p-queue 9.3.3
- p-timeout 7.0.2
- package-manager-detector 1.8.0
- picomatch 2.3.2
- picomatch 4.0.7
- postcss 8.5.28
- prismjs 1.30.0
- process-ancestry 0.1.0
- property-information 7.2.0
- radix3 1.1.2
- react 19.3.0
- react-dom 19.3.0
- react-refresh 0.18.0
- react-remove-scroll 2.7.2
- react-remove-scroll-bar 2.3.8
- react-style-singleton 2.2.3
- readdirp 5.1.1
- regex 6.1.0
- regex-recursion 6.0.2
- regex-utilities 2.3.0
- resolve-pkg-maps 1.0.0
- retext-smartypants 6.2.0
- rolldown 1.2.8
- satteri 0.10.5
- scheduler 0.28.0
- shiki 4.4.3
- sisteransi 1.0.5
- sitemap 9.0.1
- space-separated-tokens 2.0.2
- stringify-entities 4.0.4
- svgo 4.1.0
- tailwind-merge 3.7.0
- tailwindcss 4.3.3
- tapable 2.3.3
- tiny-inflate 1.0.3
- tinyclip 0.1.15
- tinyexec 1.3.0
- tinyglobby 0.2.17
- trim-lines 3.0.1
- trough 2.2.0
- ufo 1.6.4
- ultrahtml 1.7.0
- uncrypto 0.1.3
- undici 8.11.2
- undici-types 7.18.2
- undici-types 8.3.0
- unified 11.0.5
- unifont 0.7.5
- unist-util-is 6.0.1
- unist-util-position 5.0.0
- unist-util-stringify-position 4.0.0
- unist-util-visit 5.1.0
- unist-util-visit-parents 6.0.2
- unstorage 1.17.5
- update-browserslist-db 1.3.3
- use-callback-ref 1.3.3
- use-sidecar 1.1.3
- vfile 6.0.3
- vfile-message 4.0.3
- vite 8.3.0
- vitefu 1.1.3
- xxhash-wasm 1.1.0
- yocto-queue 1.2.2
- zod 4.6.2
- zwitch 2.0.4

### MPL-2.0 (24)

- lightningcss 1.32.0
- lightningcss 1.33.0
- lightningcss-android-arm64 1.32.0
- lightningcss-android-arm64 1.33.0
- lightningcss-darwin-arm64 1.32.0
- lightningcss-darwin-arm64 1.33.0
- lightningcss-darwin-x64 1.32.0
- lightningcss-darwin-x64 1.33.0
- lightningcss-freebsd-x64 1.32.0
- lightningcss-freebsd-x64 1.33.0
- lightningcss-linux-arm-gnueabihf 1.32.0
- lightningcss-linux-arm-gnueabihf 1.33.0
- lightningcss-linux-arm64-gnu 1.32.0
- lightningcss-linux-arm64-gnu 1.33.0
- lightningcss-linux-arm64-musl 1.32.0
- lightningcss-linux-arm64-musl 1.33.0
- lightningcss-linux-x64-gnu 1.32.0
- lightningcss-linux-x64-gnu 1.33.0
- lightningcss-linux-x64-musl 1.32.0
- lightningcss-linux-x64-musl 1.33.0
- lightningcss-win32-arm64-msvc 1.32.0
- lightningcss-win32-arm64-msvc 1.33.0
- lightningcss-win32-x64-msvc 1.32.0
- lightningcss-win32-x64-msvc 1.33.0

### Python-2.0 (1)

- argparse 2.0.1

## The SDKs

The Rust SDK, `crates/mcpd-sdk`, is a workspace member and its dependencies
are in the crate list above.

- **TypeScript**: no third-party runtime dependency. `sdks/typescript/package.json` declares none.
- **Python**: no third-party runtime dependency. `sdks/python/pyproject.toml` declares none.
- **Go**: no third-party runtime dependency. `sdks/go/go.mod` declares none.
- **Elixir**: no third-party runtime dependency. `sdks/elixir/mix.exs` declares none.
- **Kotlin**: the artefacts below, declared in `sdks/kotlin/build.gradle.kts`.
  Gradle writes no lock file, so their transitive dependencies are not
  enumerated here.

  - org.jetbrains.kotlinx:kotlinx-serialization-json 1.11.0, Apache-2.0
  - org.jetbrains.kotlinx:kotlinx-coroutines-core 1.11.0, Apache-2.0

