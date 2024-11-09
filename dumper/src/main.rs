#![no_std]
#![no_main]

use alloc::ffi::CString;
use alloc::format;
use core::arch::global_asm;
use core::cmp::min;
use core::ffi::{c_int, CStr};
use core::mem::zeroed;
use core::panic::PanicInfo;
use obfw::FirmwareDump;
use okf::fd::{openat, write_all, OpenFlags, AT_FDCWD};
use okf::lock::MtxLock;
use okf::mount::{Filesystem, Mount};
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
    let path = unsafe { CString::from_vec_unchecked(DUMP_FILE.as_bytes().to_vec()) };
    let flags = OpenFlags::O_WRONLY | OpenFlags::O_CREAT | OpenFlags::O_TRUNC;
    let fd = match unsafe { openat(k, AT_FDCWD, path.as_ptr(), UioSeg::Kernel, flags, 0o777) } {
        Ok(v) => v,
        Err(e) => {
            let m = format!("Could not open {} ({})", DUMP_FILE, c_int::from(e));
            notify(k, &m);
            return;
        }
    };

    // Write magic.
    if !write_dump(k, fd.as_raw_fd(), FirmwareDump::<()>::MAGIC) {
        return;
    }

    // Lock mount list.
    let mtx = k.var(K::MOUNTLIST_MTX);

    unsafe { k.mtx_lock_flags(mtx.ptr(), 0, c"".as_ptr(), 0) };

    // Dump all read-only mounts.
    let list = k.var(K::MOUNTLIST);
    let mut mp = unsafe { (*list.ptr()).first };
    let mut ok = true;

    while !mp.is_null() {
        // vfs_busy always success without MBF_NOWAIT.
        unsafe { k.vfs_busy(mp, K::MBF_MNTLSTLOCK) };

        // Check if read-only.
        let lock = unsafe { MtxLock::new(k, (*mp).mtx()) };

        ok = if unsafe { (*mp).flags() & K::MNT_RDONLY != 0 } {
            unsafe { dump_mount(k, fd.as_raw_fd(), mp, lock) }
        } else {
            drop(lock);
            true
        };

        // vfs_busy with MBF_MNTLSTLOCK will unlock before return so we need to re-acquire the lock.
        unsafe { k.mtx_lock_flags(mtx.ptr(), 0, c"".as_ptr(), 0) };

        // This need to be done inside mountlist_mtx otherwise our current mp may be freed when we
        // try to access the next mount point.
        unsafe { k.vfs_unbusy(mp) };

        if !ok {
            break;
        }

        mp = unsafe { (*mp).entry().next };
    }

    unsafe { k.mtx_unlock_flags(mtx.ptr(), 0, c"".as_ptr(), 0) };

    if !ok {
        return;
    }

    // Write end entry.
    if !write_dump(
        k,
        fd.as_raw_fd(),
        core::slice::from_ref(&FirmwareDump::<()>::ITEM_END),
    ) {
        return;
    }

    // Flush data.
    let td = K::Pcpu::curthread();
    let errno = unsafe { k.kern_fsync(td, fd.as_raw_fd(), 1) };

    if errno != 0 {
        let m = format!("Couldn't flush data to {} ({})", DUMP_FILE, errno);
        notify(k, &m);
        return;
    }

    // Notify the user.
    notify(k, "Dump completed!");
}

unsafe fn dump_mount<K: Kernel>(k: K, fd: c_int, mp: *mut K::Mount, lock: MtxLock<K>) -> bool {
    drop(lock);

    // Check filesystem type.
    let fs = (*mp).fs();
    let name = CStr::from_ptr((*fs).name()).to_bytes();

    if !matches!(name, b"exfatfs" | b"ufs") {
        return true;
    }

    // Write header.
    if !write_dump(
        k,
        fd,
        core::slice::from_ref(&FirmwareDump::<()>::ITEM_PARTITION),
    ) {
        return false;
    }

    true
}

#[inline(never)]
fn write_dump<K: Kernel>(k: K, fd: c_int, data: &[u8]) -> bool {
    let td = K::Pcpu::curthread();

    match unsafe { write_all(k, fd, data, UioSeg::Kernel, td) } {
        Ok(_) => true,
        Err(e) => {
            let m = format!("Could not write {} ({})", DUMP_FILE, c_int::from(e));
            notify(k, &m);
            false
        }
    }
}

#[inline(never)]
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
static DUMP_FILE: &str = "/mnt/usb0/firmware.obf";
