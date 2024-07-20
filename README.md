# Firmware Dumper

This is a kernel-mode payload to dump PS4 system files required by Obliteration. Only 11.00 is currently supported.

## Building from source

### Prerequisites

- Rust on nightly channel
- Python 3

### Install additional Rust component

```sh
rustup component add --toolchain nightly rust-src llvm-tools
```

### Build

```sh
./build.py
```

## Development

You need to install `x86_64-unknown-none` for rust-analyzer to work correctly:

```sh
rustup target add x86_64-unknown-none
```

## License

MIT
