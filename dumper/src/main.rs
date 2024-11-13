#![no_std]
#![no_main]

use alloc::collections::vec_deque::VecDeque;
use alloc::ffi::CString;
use alloc::format;
use core::arch::global_asm;
use core::cmp::min;
use core::ffi::{c_int, CStr};
use core::hint::unreachable_unchecked;
use core::mem::{zeroed, MaybeUninit};
use core::panic::PanicInfo;
use core::ptr::null_mut;
use obfw::FirmwareDump;
use okf::fd::{openat, write_all, OpenFlags, AT_FDCWD};
use okf::lock::MtxLock;
use okf::mount::{Filesystem, FsOps, FsStats, Mount};
use okf::namei::ComponentName;
use okf::pcpu::Pcpu;
use okf::thread::Thread;
use okf::uio::{IoVec, Uio, UioSeg};
use okf::vnode::{DirEnt, Vnode, VopLookup, VopReadDir};
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
    let fs = CStr::from_ptr((*fs).name()).to_bytes();

    if !matches!(fs, b"exfatfs" | b"ufs") {
        return true;
    }

    // All interested partition should have mounted from as a valid UTF-8.
    let stats = (*mp).stats();
    let path = match CStr::from_ptr((*stats).mounted_from()).to_str() {
        Ok(v) => v,
        Err(_) => return true,
    };

    // Write entry type.
    if !write_dump(k, fd, &[FirmwareDump::<()>::ITEM_PARTITION]) {
        return false;
    }

    // Write entry version.
    if !write_dump(k, fd, &[0]) {
        return false;
    }

    // Write filesystem type.
    if !write_dump(k, fd, &fs.len().to_le_bytes()) || !write_dump(k, fd, fs) {
        return false;
    }

    // Write mounted from.
    if !write_dump(k, fd, &path.len().to_le_bytes()) || !write_dump(k, fd, path.as_bytes()) {
        return false;
    }

    // Get root vnode.
    let vp = match (*mp).ops().root(mp, K::LK_SHARED) {
        Ok(v) => v,
        Err(e) => {
            notify(k, &format!("Couldn't get root vnode of {} ({})", path, e));
            return false;
        }
    };

    // Dump all vnodes.
    let mut dirs = VecDeque::from([vp]);

    while let Some(vp) = dirs.pop_front() {
        // Check type.
        let ty = (*vp).ty();
        let ok = if ty == K::VDIR {
            list_files(k, vp)
        } else {
            let m = format!("Unknown vnode {ty}");
            notify(k, &m);
            false
        };

        // Release vnode.
        k.vput(vp);

        if !ok {
            return false;
        }
    }

    true
}

unsafe fn list_files<K: Kernel>(k: K, vp: *mut K::Vnode) -> bool {
    let td = K::Pcpu::curthread();
    let mut off = 0;

    loop {
        // Setup output buffer.
        let mut buf = MaybeUninit::<DirEnt<256>>::uninit();
        let mut vec = IoVec {
            ptr: buf.as_mut_ptr().cast(),
            len: size_of_val(&buf),
        };

        // Setup argument.
        let mut io = Uio::read(&mut vec, off, td).unwrap();
        let mut eof = MaybeUninit::uninit();
        let mut args = VopReadDir::new(
            k,
            vp,
            &mut io,
            (*td).cred(),
            eof.as_mut_ptr(),
            null_mut(),
            null_mut(),
        );

        // Read entry.
        let errno = k.vop_readdir((*vp).ops(), &mut args);

        if errno != 0 {
            let m = format!("Couldn't read directory entry ({errno})");
            notify(k, &m);
            return false;
        }

        off = io.offset().try_into().unwrap();

        // Parse entries.
        let len = size_of_val(&buf) - usize::try_from(io.remaining()).unwrap();
        let mut buf = core::slice::from_raw_parts_mut::<u8>(buf.as_mut_ptr().cast(), len);

        while !buf.is_empty() {
            // Get entry and move to next one.
            let ent = buf.as_mut_ptr() as *mut DirEnt<1>;
            let len: usize = (*ent).len.into();

            buf = &mut buf[len..];

            // Skip "." and "..".
            let len = (*ent).name_len.into();
            let name = core::slice::from_raw_parts::<u8>((*ent).name.as_ptr().cast(), len);

            if matches!(name, b"." | b"..") {
                continue;
            }

            // Lookup.
            let mut child = MaybeUninit::uninit();
            let name = (*ent).name.as_mut_ptr();
            let mut cn = ComponentName::new(k, K::LOOKUP, K::LK_SHARED, name, td);
            let mut args = VopLookup::new(k, vp, child.as_mut_ptr(), &mut cn);
            let errno = k.vop_lookup((*vp).ops(), &mut args);

            if errno != 0 {
                let m = format!("Couldn't lookup a child ({errno})");
                notify(k, &m);
                return false;
            }

            // Keep vnode.
            let child = child.assume_init();

            k.vput(child);
        }

        // Stop if no more entries.
        if eof.assume_init() != 0 {
            break;
        }
    }

    true
}

#[inline(never)]
fn write_dump<K: Kernel>(k: K, fd: c_int, data: &[u8]) -> bool {
    let td = K::Pcpu::curthread();

    match unsafe { write_all(k, fd, data, td) } {
        Ok(_) => true,
        Err(e) => {
            let m = format!("Couldn't write {} ({})", DUMP_FILE, c_int::from(e));
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

    unsafe { write_all(k, fd.as_raw_fd(), data, td).ok() };
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    // Nothing to do here since we enabled panic_immediate_abort.
    unsafe { unreachable_unchecked() };
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
