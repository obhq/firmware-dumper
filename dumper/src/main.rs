#![no_std]
#![no_main]

use alloc::format;
use core::arch::global_asm;
use core::cmp::min;
use core::ffi::c_int;
use core::mem::zeroed;
use core::panic::PanicInfo;
use okf::fd::{openat, write_all, OpenFlags, AT_FDCWD};
use okf::lock::MtxLock;
use okf::mount::Mount;
use okf::pcpu::Pcpu;
use okf::uio::UioSeg;
use okf::{kernel, Allocator, Kernel};

extern crate alloc;

// The job of this custom entry point is:
//
// - Get address where our payload is loaded.
// - Do ELF relocation on our payload.
global_asm!(
    ".globl _start",
    ".section .text.startup",
    "_start:",
    "lea rdi, [rip]",
    "sub rdi, 7", // 7 is size of the above "lea rdi, [rip]".
    "mov rax, rdi",
    "add rax, 0x80", // Offset of dynamic section configured in linker script.
    "xor r8, r8",
    "0:",
    "mov rsi, [rax]",
    "mov rcx, [rax+8]",
    "add rax, 16",
    "test rsi, rsi", // Check if DT_NULL.
    "jz 1f",
    "cmp rsi, 7", // Check if DT_RELA.
    "jz 2f",
    "cmp rsi, 8", // Check if DT_RELASZ.
    "jz 3f",
    "jmp 0b",
    "2:", // Keep DT_RELA.
    "mov rdx, rdi",
    "add rdx, rcx",
    "jmp 0b",
    "3:", // Keep DT_RELASZ.
    "mov r8, rcx",
    "jmp 0b",
    "1:",
    "test r8, r8", // Check if no more DT_RELA entries.
    "jz main",
    "mov rsi, [rdx]",
    "mov rax, [rdx+8]",
    "mov rcx, [rdx+16]",
    "add rdx, 24",
    "sub r8, 24",
    "test eax, eax", // Check if R_X86_64_NONE.
    "jz main",
    "cmp eax, 8", // Check if R_X86_64_RELATIVE.
    "jnz 1b",
    "add rsi, rdi",
    "add rcx, rdi",
    "mov [rsi], rcx",
    "jmp 1b",
);

#[no_mangle]
extern "C" fn main(_: *const u8) {
    run(<kernel!()>::default())
}

fn run<K: Kernel>(k: K) {
    // Create dump file.
    let path = c"/mnt/usb0/firmware.obf";
    let flags = OpenFlags::O_WRONLY | OpenFlags::O_CREAT | OpenFlags::O_TRUNC;
    let fd = match unsafe { openat(k, AT_FDCWD, path.as_ptr(), UioSeg::Kernel, flags, 0o777) } {
        Ok(v) => v,
        Err(e) => {
            notify(
                k,
                &format!("Could not open /mnt/usb0/firmware.obf ({})", c_int::from(e)),
            );
            return;
        }
    };

    // Get target mounts.
    let mtx = k.var(K::MOUNTLIST_MTX);
    let lock = unsafe { MtxLock::new(k, mtx.ptr()) };
    let list = k.var(K::MOUNTLIST);
    let mut mp = unsafe { (*list.ptr()).first };

    while !mp.is_null() {
        mp = unsafe { (*mp).entry().next };
    }

    drop(lock);
    drop(fd);

    // Notify the user.
    notify(k, "Dump completed!");
}

fn notify<K: Kernel>(k: K, msg: &str) {
    // Open notification device.
    let devs = [c"/dev/notification0", c"/dev/notification1"];
    let mut fd = None;

    for dev in devs.into_iter().map(|v| v.as_ptr()) {
        if let Ok(v) = unsafe { openat(k, AT_FDCWD, dev, UioSeg::Kernel, OpenFlags::O_WRONLY, 0) } {
            fd = Some(v);
            break;
        }
    }

    // Check if we have a device to write to.
    let fd = match fd {
        Some(v) => v,
        None => return,
    };

    // Setup notification.
    let mut data: OrbisNotificationRequest = unsafe { zeroed() };
    let msg = msg.as_bytes();
    let len = min(data.message.len() - 1, msg.len());

    data.target_id = -1;
    data.use_icon_image_uri = 1;
    data.message[..len].copy_from_slice(&msg[..len]);

    // Write notification.
    let len = size_of_val(&data);
    let data = &data as *const OrbisNotificationRequest as *const u8;
    let data = unsafe { core::slice::from_raw_parts(data, len) };
    let td = K::Pcpu::curthread();

    unsafe { write_all(k, fd.as_raw_fd(), data, UioSeg::Kernel, td).ok() };
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    // Nothing to do here since we enabled panic_immediate_abort.
    loop {}
}

/// By OSM-Made.
#[repr(C)]
struct OrbisNotificationRequest {
    ty: c_int,
    req_id: c_int,
    priority: c_int,
    msg_id: c_int,
    target_id: c_int,
    user_id: c_int,
    unk1: c_int,
    unk2: c_int,
    app_id: c_int,
    error_num: c_int,
    unk3: c_int,
    use_icon_image_uri: u8,
    message: [u8; 1024],
    icon_uri: [u8; 1024],
    unk: [u8; 1024],
}

#[global_allocator]
static ALLOCATOR: Allocator<kernel!()> = Allocator::new();
