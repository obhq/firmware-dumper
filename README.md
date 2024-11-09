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

The payload must not exceed 0x4000 bytes due to limitation of PPPwn. It might be possible to increase this limit but I have not tried yet. AFAIK the only possible issues for increasing this limitation is it have more chance for UDP fragmentation to be out of order on the kernel side.

## License

MIT
