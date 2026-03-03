# slsk-rs

A Soulseek P2P client and protocol library written in Rust.

## Crates

| Crate | Description |
|---|---|
| [`slsk`](crates/slsk/) | Terminal UI client for the Soulseek network |
| [`slsk-protocol`](crates/slsk-protocol/) | Pure-Rust implementation of the Soulseek protocol |

## Requirements

- Rust 1.85+ (edition 2024)
- A Soulseek account — register at [slsknet.org](https://www.slsknet.org/)

## Getting started

```sh
git clone https://github.com/aeyoll/slsk-rs
cd slsk-rs

export SOULSEEK_USERNAME=youruser
export SOULSEEK_PASSWORD=yourpassword

cargo run -p slsk
```

## License

MIT
