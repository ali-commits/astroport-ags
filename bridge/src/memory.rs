//! Donation plan, owned-range list, owns(), adoption check (design.md 5.1).

use crate::abi::AstroportHostApi;

/// Unmapped from AnyPS5's arena before Kyty starts: the PS5 system-managed area, and general
/// room directly below Kyty's kAmmWindowBase.
pub const DONATED: [(usize, usize); 2] = [(0x2_0000_0000, 0x7_FFFF_0000), (0xDC_0000_0000, 0xEC_0000_0000)];

/// What Kyty holds: the donated ranges plus the system-reserved range the arena never maps.
pub const OWNED: [(usize, usize); 3] = [DONATED[0], DONATED[1], (0x7_FFFF_C000, 0x10_0000_0000)];

pub fn owns(address: usize, bytes: usize) -> bool {
    OWNED.iter().any(|&(base, end)| address >= base && address.checked_add(bytes).is_some_and(|last| last <= end))
}

pub extern "C" fn owns_c(address: usize, bytes: usize) -> bool {
    owns(address, bytes)
}

/// # Safety
/// `host.donate` must be callable.
pub unsafe fn donate(host: &AstroportHostApi) -> Result<(), String> {
    for (base, end) in DONATED {
        if (host.donate.unwrap())(base as u64, (end - base) as u64) != base as u64 {
            return Err(format!("donation of {base:#x}..{end:#x} refused by the host"));
        }
    }
    Ok(())
}

/// True when something is mapped at `address`: a fixed no-replace mapping must fail with EEXIST.
pub fn mapped(address: usize) -> bool {
    use libc::{MAP_ANONYMOUS, MAP_FIXED_NOREPLACE, MAP_PRIVATE};
    unsafe {
        let flags = MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE;
        let probe = libc::mmap(address as *mut _, 4096, libc::PROT_NONE, flags, -1, 0);
        if probe == libc::MAP_FAILED {
            return *libc::__errno_location() == libc::EEXIST;
        }
        libc::munmap(probe, 4096);
        false
    }
}

/// After `hooks.reserved()`: Kyty must hold the start of every owned range.
pub fn verify_adopted() -> Result<(), String> {
    match OWNED.iter().find(|&&(base, _)| !mapped(base)) {
        Some((base, end)) => Err(format!("Kyty did not adopt {base:#x}..{end:#x}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owns_is_inside_one_range() {
        assert!(owns(0x3_0000_0000, 0x8000_0000));
        assert!(owns(0xDC_0000_0000, 1));
        assert!(owns(0xEB_FFFF_F000, 0x1000));
        assert!(owns(0xF_E000_0000, 0x2000_0000));
        assert!(!owns(0xEB_FFFF_F000, 0x1001));
        assert!(!owns(0x7_FFFF_0000, 1));
        assert!(!owns(0x1_FFFF_F000, 0x2000));
        assert!(!owns(0x40_0000, 1));
        assert!(!owns(usize::MAX, 2));
    }

    #[test]
    fn mapped_sees_a_mapping_and_leaves_a_hole_free() {
        unsafe {
            // far from where the kernel places the mappings of other test threads
            let flags = libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE;
            let page = libc::mmap(0x6000_0000_0000usize as *mut _, 8192, libc::PROT_NONE, flags, -1, 0);
            assert_ne!(page, libc::MAP_FAILED);
            assert!(mapped(page as usize));
            libc::munmap(page, 8192);
            assert!(!mapped(page as usize));
            assert!(!mapped(page as usize), "the probe must not stay mapped");
        }
    }
}
