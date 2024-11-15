# Firmware Dumper

This is a kernel-mode payload to dump PS4 system files required by Obliteration. Only 11.00 is currently supported.

## Setup

Plug a USB drive to the PS4 and make sure the PS4 can write some files to it. You can test this by copy some game screenshots to it to see if it success without any errors.

## Running

You need to use TheFloW [PPPwn](https://github.com/TheOfficialFloW/PPPwn) with `--stage2` pointed to `firmware-dumper.bin` like the following:

```sh
sudo python3 pppwn.py --interface=enp0s3 --fw=1100 --stage2=firmware-dumper.bin
```

Wait for a notification `Dump completed!`. This can take up to 10 minutes depend on how fast is your USB drive then shutdown the PS4 (not putting it into rest mode). Once the PS4 completely shutdown unplug the USB drive to grab `firmware.obf`.

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
