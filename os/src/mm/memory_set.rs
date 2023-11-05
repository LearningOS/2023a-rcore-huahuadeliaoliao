//! Implementation of [`MapArea`] and [`MemorySet`].
use super::{frame_alloc, FrameTracker};
use super::{PTEFlags, PageTable, PageTableEntry};
use super::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum};
use super::{StepByOne, VPNRange};
use crate::config::{MEMORY_END, PAGE_SIZE, TRAMPOLINE, TRAP_CONTEXT_BASE, USER_STACK_SIZE};
use crate::sync::UPSafeCell;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::arch::asm;
use lazy_static::*;
use riscv::register::satp;
use super::frame_allocator::MemoryError;
use alloc::format;
use iset::IntervalMap;
use core::ops::Range;

extern "C" {
    fn stext();
    fn etext();
    fn srodata();
    fn erodata();
    fn sdata();
    fn edata();
    fn sbss_with_stack();
    fn ebss();
    fn ekernel();
    fn strampoline();
}

lazy_static! {
    /// The kernel's initial memory mapping(kernel address space)
    pub static ref KERNEL_SPACE: Arc<UPSafeCell<MemorySet>> =
        Arc::new(unsafe { UPSafeCell::new(MemorySet::new_kernel()) });
}
/// address space
pub struct MemorySet {
    page_table: PageTable,
    // areas:Vec<MapArea>
    // use interval map to store the areas
    areas: IntervalMap<VirtPageNum,MapArea>,
}

impl MemorySet {
    /// Create a new empty `MemorySet`.
    pub fn new_bare() -> Self {
        Self {
            page_table: PageTable::new(),
            areas: IntervalMap::new(),
        }
    }
    /// Get the page table token
    pub fn token(&self) -> usize {
        self.page_table.token()
    }
    /// Assume that no conflicts.
    pub fn insert_framed_area(
        &mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission,
    ) {
        self.push(
            MapArea::new(start_va, end_va, MapType::Framed, permission),
            None,
        );
    }
    /// remove a area
    pub fn remove_area_with_start_vpn(&mut self, start_vpn: VirtPageNum) {
        if let Some((idx, area)) = self
            .areas
            .iter_mut(..)
            .find(|(_, area)| area.vpn_range.get_start() == start_vpn)
        {
            area.unmap(&mut self.page_table);
            self.areas.remove(idx);
        }
    }
    /// Add a new MapArea into this MemorySet.
    /// Assuming that there are no conflicts in the virtual address
    /// space.
    fn push(&mut self, mut map_area: MapArea, data: Option<&[u8]>) {
        let vpn_range = map_area.vpn_range;
        let vpn_start = vpn_range.get_start();
        let vpn_end = vpn_range.get_end();
        map_area.map(&mut self.page_table).expect(
            format!("[Kernel]: Fail to allocate Frames from {:x} to {:x}",
            vpn_start.0, vpn_end.0).as_str());
        if let Some(data) = data {
            map_area.copy_data(&mut self.page_table, data);
        }
        if vpn_start < vpn_end {
            self.areas.insert(vpn_start..vpn_end, map_area);
            }
    }
    /// Mention that trampoline is not collected by areas.
    fn map_trampoline(&mut self) {
        self.page_table.map(
            VirtAddr::from(TRAMPOLINE).into(),
            PhysAddr::from(strampoline as usize).into(),
            PTEFlags::R | PTEFlags::X,
        );
    }
    /// Without kernel stacks.
    pub fn new_kernel() -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map kernel sections
        info!(".text [{:#x}, {:#x})", stext as usize, etext as usize);
        info!(".rodata [{:#x}, {:#x})", srodata as usize, erodata as usize);
        info!(".data [{:#x}, {:#x})", sdata as usize, edata as usize);
        info!(
            ".bss [{:#x}, {:#x})",
            sbss_with_stack as usize, ebss as usize
        );
        info!("mapping .text section");
        memory_set.push(
            MapArea::new(
                (stext as usize).into(),
                (etext as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::X,
            ),
            None,
        );
        info!("mapping .rodata section");
        memory_set.push(
            MapArea::new(
                (srodata as usize).into(),
                (erodata as usize).into(),
                MapType::Identical,
                MapPermission::R,
            ),
            None,
        );
        info!("mapping .data section");
        memory_set.push(
            MapArea::new(
                (sdata as usize).into(),
                (edata as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping .bss section");
        memory_set.push(
            MapArea::new(
                (sbss_with_stack as usize).into(),
                (ebss as usize).into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        info!("mapping physical memory");
        memory_set.push(
            MapArea::new(
                (ekernel as usize).into(),
                MEMORY_END.into(),
                MapType::Identical,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        memory_set
    }
    /// Include sections in elf and trampoline and TrapContext and user stack,
    /// also returns user_sp_base and entry point.
    pub fn from_elf(elf_data: &[u8]) -> (Self, usize, usize) {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // map program headers of elf, with U flag
        let elf = xmas_elf::ElfFile::new(elf_data).unwrap();
        let elf_header = elf.header;
        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");
        let ph_count = elf_header.pt2.ph_count();
        let mut max_end_vpn = VirtPageNum(0);
        for i in 0..ph_count {
            let ph = elf.program_header(i).unwrap();
            if ph.get_type().unwrap() == xmas_elf::program::Type::Load {
                let start_va: VirtAddr = (ph.virtual_addr() as usize).into();
                let end_va: VirtAddr = ((ph.virtual_addr() + ph.mem_size()) as usize).into();
                let mut map_perm = MapPermission::U;
                let ph_flags = ph.flags();
                if ph_flags.is_read() {
                    map_perm |= MapPermission::R;
                }
                if ph_flags.is_write() {
                    map_perm |= MapPermission::W;
                }
                if ph_flags.is_execute() {
                    map_perm |= MapPermission::X;
                }
                let map_area = MapArea::new(start_va, end_va, MapType::Framed, map_perm);
                max_end_vpn = map_area.vpn_range.get_end();
                memory_set.push(
                    map_area,
                    Some(&elf.input[ph.offset() as usize..(ph.offset() + ph.file_size()) as usize]),
                );
            }
        }
        // map user stack with U flags
        let max_end_va: VirtAddr = max_end_vpn.into();
        let mut user_stack_bottom: usize = max_end_va.into();
        // guard page
        user_stack_bottom += PAGE_SIZE;
        let user_stack_top = user_stack_bottom + USER_STACK_SIZE;
        memory_set.push(
            MapArea::new(
                user_stack_bottom.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // used in sbrk
        memory_set.push(
            MapArea::new(
                user_stack_top.into(),
                user_stack_top.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W | MapPermission::U,
            ),
            None,
        );
        // map TrapContext
        memory_set.push(
            MapArea::new(
                TRAP_CONTEXT_BASE.into(),
                TRAMPOLINE.into(),
                MapType::Framed,
                MapPermission::R | MapPermission::W,
            ),
            None,
        );
        (
            memory_set,
            user_stack_top,
            elf.header.pt2.entry_point() as usize,
        )
    }
    /// Create a new address space by copy code&data from a exited process's address space.
    pub fn from_existed_user(user_space: &Self) -> Self {
        let mut memory_set = Self::new_bare();
        // map trampoline
        memory_set.map_trampoline();
        // copy data sections/trap_context/user_stack
        for area in user_space.areas.values(..) {
            let new_area = MapArea::from_another(area);
            memory_set.push(new_area, None);
            // copy data from another space
            for vpn in area.vpn_range {
                let src_ppn = user_space.translate(vpn).unwrap().ppn();
                let dst_ppn = memory_set.translate(vpn).unwrap().ppn();
                dst_ppn
                    .get_bytes_array()
                    .copy_from_slice(src_ppn.get_bytes_array());
            }
        }
        memory_set
    }
    /// Change page table by writing satp CSR Register.
    pub fn activate(&self) {
        let satp = self.page_table.token();
        unsafe {
            satp::write(satp);
            asm!("sfence.vma");
        }
    }
    /// Translate a virtual page number to a page table entry
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.page_table.translate(vpn)
    }

    ///Remove all `MapArea`
    pub fn recycle_data_pages(&mut self) {
        self.areas.clear();
    }

    /// shrink the area to new_end
    #[allow(unused)]
    pub fn shrink_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self.areas
            .values_mut(..)
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.shrink_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// append the area to new_end
    #[allow(unused)]
    pub fn append_to(&mut self, start: VirtAddr, new_end: VirtAddr) -> bool {
        if let Some(area) = self
            .areas
            .values_mut(..)
            .find(|area| area.vpn_range.get_start() == start.floor())
        {
            area.append_to(&mut self.page_table, new_end.ceil());
            true
        } else {
            false
        }
    }

    /// map a new area
    pub fn map_area(&mut self,
        start_va: VirtAddr,
        end_va: VirtAddr,
        permission: MapPermission) -> bool {
        if let Some(t) = self
            .areas
            .values(start_va.into()..end_va.ceil())
            .next() {
                println!("[kernel]: map failed, overlapped interval {:?}..{:?}",
                    t.vpn_range.get_start(),t.vpn_range.get_end());
                false
        } else {
            // self.insert_framed_area(start_va, end_va, permission);
            let mut map_area = MapArea::new(start_va, end_va, MapType::Framed, permission);
            match map_area.map(&mut self.page_table) {
                Ok(_) => {
                    self.areas.insert(
                        map_area.vpn_range.get_start()..map_area.vpn_range.get_end(),
                        map_area);
                    true
                }
                Err(_) => {
                    println!("[kernel]: map failed, no enough memory for {:?}..{:?}",start_va, end_va);
                    false
                }
            }
        }
    }

    /// check if a Vec of VPNRange contiously contain start_vpn..end_vpn
    /// return false if overlapped_areas not contiously contain start_vpn..end_vpn
    /// or overlapped_areas is empty
    fn continiously_contain(
        overlapped_areas:&Vec<Range<VirtPageNum>>,
        start_vpn: VirtPageNum,
        end_vpn: VirtPageNum) -> bool{
        match overlapped_areas.len() {
            0 => false,
            1 => {
                let first = &overlapped_areas[0];
                if start_vpn < first.start || end_vpn > first.end {
                    false
                }
                else {
                    true
                }
            },
            n => {
                let first = &overlapped_areas[0];
                let last = &overlapped_areas[n-1];
                if start_vpn < first.start || end_vpn > last.end {
                    return false;
                }
                let mut vec_end_vpn = first.end;
                for i in 1..n {
                    let next = &overlapped_areas[i];
                    if vec_end_vpn != next.start {
                        return false
                    }
                    vec_end_vpn = next.end;
                }
                true
            }
        }
    }

    /// split a contious Maparea into three pieces (head_area, mid_area_vector, tail_area)
    fn split_into_three(mut area_vec: Vec<MapArea>,start_vpn: VirtPageNum, end_vpn: VirtPageNum) -> (MapArea, Vec<MapArea>, MapArea) {
        assert!(area_vec.len() > 0);
        let mut mid_areas = Vec::new();
        match area_vec.len() {
            1 => {
                let range = VPNRange::new(start_vpn, end_vpn);
                let (left, mid, right) = area_vec.remove(0).split_by_range(range);
                mid_areas.push(mid);
                (left, mid_areas, right)
            }
            _ => {
                // split the first area
                let range = VPNRange::new(start_vpn, area_vec[0].vpn_range.get_end());
                let (left, to_unmap, _) = area_vec.remove(0).split_by_range(range);
                mid_areas.push(to_unmap);
                let range = VPNRange::new(area_vec.last().unwrap().vpn_range.get_start(), end_vpn);
                let (_, to_unmap, right) = area_vec.pop().unwrap().split_by_range(range);
                mid_areas.push(to_unmap);
                for i in area_vec {
                    mid_areas.push(i);
                }
                (left, mid_areas, right)
            }
        }
    }

    /// unmap a area
    pub fn unmap_area(&mut self, start_va: VirtAddr, end_va: VirtAddr) -> bool {
        let start_vpn = start_va.into();
        let end_vpn = end_va.ceil();
        // get overlapped areas which is sorted by start vpn
        let overlapped_intervals:Vec<_> = self.areas.intervals(start_vpn..end_vpn).collect();

        if Self::continiously_contain(&overlapped_intervals,start_vpn,end_vpn) {
            let overlapped_items:Vec<_> = overlapped_intervals
                .into_iter()
                .map(|x| self.areas.remove(x).unwrap())
                .collect();
            let (head, mut to_unmap, tail) = Self::split_into_three(overlapped_items, start_vpn, end_vpn);

            for area in to_unmap.iter_mut() {
                area.unmap(&mut self.page_table);
            }
            drop(to_unmap);

            let (l, r) = (head.vpn_range.get_start(),head.vpn_range.get_end());
            if l != r {
                self.areas.insert(l..r, head);
            }

            let (l, r) = (tail.vpn_range.get_start(),tail.vpn_range.get_end());
            if l != r {
                self.areas.insert(l..r, tail);
            }
            true
        } else {
            false
        }
    }
}

/// map area structure, controls a contiguous piece of virtual memory
pub struct MapArea {
    vpn_range: VPNRange,
    data_frames: BTreeMap<VirtPageNum, FrameTracker>,
    map_type: MapType,
    map_perm: MapPermission,
}

impl MapArea {
    pub fn new(
        start_va: VirtAddr,
        end_va: VirtAddr,
        map_type: MapType,
        map_perm: MapPermission,
    ) -> Self {
        let start_vpn: VirtPageNum = start_va.floor();
        let end_vpn: VirtPageNum = end_va.ceil();
        Self {
            vpn_range: VPNRange::new(start_vpn, end_vpn),
            data_frames: BTreeMap::new(),
            map_type,
            map_perm,
        }
    }
    pub fn from_another(another: &Self) -> Self {
        Self {
            vpn_range: VPNRange::new(another.vpn_range.get_start(), another.vpn_range.get_end()),
            data_frames: BTreeMap::new(),
            map_type: another.map_type,
            map_perm: another.map_perm,
        }
    }
    pub fn map_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) -> Result<(), MemoryError> {
        let ppn: PhysPageNum;
        match self.map_type {
            MapType::Identical => {
                ppn = PhysPageNum(vpn.0);
            }
            MapType::Framed => {
                if let Some(frame) = frame_alloc() {
                    ppn = frame.ppn;
                    self.data_frames.insert(vpn, frame);
                }
                else {
                    return Err(MemoryError::FrameAllocate);
                }
            }
        }
        let pte_flags = PTEFlags::from_bits(self.map_perm.bits).unwrap();
        page_table.map(vpn, ppn, pte_flags);
        Ok(())
    }
    pub fn unmap_one(&mut self, page_table: &mut PageTable, vpn: VirtPageNum) {
        if self.map_type == MapType::Framed {
            self.data_frames.remove(&vpn);
        }
        page_table.unmap(vpn);
    }
    pub fn map(&mut self, page_table: &mut PageTable) -> Result<(), MemoryError> {
        for vpn in self.vpn_range {
            self.map_one(page_table, vpn)?
        }
        Ok(())
    }
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.vpn_range {
            self.unmap_one(page_table, vpn);
        }
    }
    #[allow(unused)]
    pub fn shrink_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(new_end, self.vpn_range.get_end()) {
            self.unmap_one(page_table, vpn)
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    #[allow(unused)]
    pub fn append_to(&mut self, page_table: &mut PageTable, new_end: VirtPageNum) {
        for vpn in VPNRange::new(self.vpn_range.get_end(), new_end) {
            self.map_one(page_table, vpn);
        }
        self.vpn_range = VPNRange::new(self.vpn_range.get_start(), new_end);
    }
    /// data: start-aligned but maybe with shorter length
    /// assume that all frames were cleared before
    pub fn copy_data(&mut self, page_table: &mut PageTable, data: &[u8]) {
        assert_eq!(self.map_type, MapType::Framed);
        let mut start: usize = 0;
        let mut current_vpn = self.vpn_range.get_start();
        let len = data.len();
        loop {
            let src = &data[start..len.min(start + PAGE_SIZE)];
            let dst = &mut page_table
                .translate(current_vpn)
                .unwrap()
                .ppn()
                .get_bytes_array()[..src.len()];
            dst.copy_from_slice(src);
            start += PAGE_SIZE;
            if start >= len {
                break;
            }
            current_vpn.step();
        }
    }

    /// split the area by range
    pub fn split_by_range(self, range: VPNRange) -> (Self, Self, Self) {
        let left = range.get_start();
        let right = range.get_end();

        let mut drop_map = BTreeMap::new();
        let mut left_map = BTreeMap::new();
        let mut right_map = BTreeMap::new();

        for (vpn, tracer) in self.data_frames {
            if vpn >= self.vpn_range.get_start() && vpn < left {
                left_map.insert(vpn, tracer);
            } else if vpn >= right && vpn < self.vpn_range.get_end() {
                right_map.insert(vpn, tracer);
            } else {
                drop_map.insert(vpn, tracer);
            }
        }
        let left_area = Self {
            vpn_range: VPNRange::new(self.vpn_range.get_start(), left),
            data_frames: left_map,
            map_type: self.map_type,
            map_perm: self.map_perm,
        };
        let drop_area = Self {
            vpn_range: VPNRange::new(left, right),
            data_frames: drop_map,
            map_type: self.map_type,
            map_perm: self.map_perm,
        };
        let right_area = Self {
            vpn_range: VPNRange::new(right, self.vpn_range.get_end()),
            data_frames: right_map,
            map_type: self.map_type,
            map_perm: self.map_perm,
        };
        (left_area, drop_area, right_area)
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
/// map type for memory set: identical or framed
pub enum MapType {
    Identical,
    Framed,
}

bitflags! {
    /// map permission corresponding to that in pte: `R W X U`
    pub struct MapPermission: u8 {
        ///Readable
        const R = 1 << 1;
        ///Writable
        const W = 1 << 2;
        ///Excutable
        const X = 1 << 3;
        ///Accessible in U mode
        const U = 1 << 4;
    }
}

impl From<u8> for MapPermission {
    fn from(value: u8) -> Self {
        MapPermission { bits: (value << 1) }
    }
}

/// remap test in kernel space
#[allow(unused)]
pub fn remap_test() {
    let mut kernel_space = KERNEL_SPACE.exclusive_access();
    let mid_text: VirtAddr = ((stext as usize + etext as usize) / 2).into();
    let mid_rodata: VirtAddr = ((srodata as usize + erodata as usize) / 2).into();
    let mid_data: VirtAddr = ((sdata as usize + edata as usize) / 2).into();
    assert!(!kernel_space
        .page_table
        .translate(mid_text.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_rodata.floor())
        .unwrap()
        .writable(),);
    assert!(!kernel_space
        .page_table
        .translate(mid_data.floor())
        .unwrap()
        .executable(),);
    println!("remap_test passed!");
}