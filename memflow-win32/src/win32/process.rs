use std::prelude::v1::*;

use super::Win32Kernel;
use crate::error::{Error, Result};
use crate::offsets::Win32ArchOffsets;
use crate::win32::VirtualReadUnicodeString;

use log::trace;
use std::fmt;

use memflow::architecture::ArchitectureObj;
use memflow::mem::{PhysicalMemory, VirtualDMA, VirtualMemory, VirtualTranslate};
use memflow::os::{
    AddressCallback, ModuleAddressCallback, ModuleAddressInfo, ModuleInfo, Process, ProcessInfo,
};
use memflow::process::{OsProcessInfo, PID};
use memflow::types::Address;

use super::Win32VirtualTranslate;

/// Exit status of a win32 process
pub type Win32ExitStatus = i32;

/// Process has not exited yet
pub const EXIT_STATUS_STILL_ACTIVE: i32 = 259;

/// EPROCESS ImageFileName byte length
pub const IMAGE_FILE_NAME_LENGTH: usize = 15;

const MAX_ITER_COUNT: usize = 65536;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize))]
pub struct Win32ModuleListInfo {
    module_base: Address,
    offsets: Win32ArchOffsets,
}

impl Win32ModuleListInfo {
    pub fn with_peb(
        mem: &mut impl VirtualMemory,
        peb: Address,
        arch: ArchitectureObj,
    ) -> Result<Win32ModuleListInfo> {
        let offsets = Win32ArchOffsets::from(arch);

        trace!("peb_ldr_offs={:x}", offsets.peb_ldr);
        trace!("ldr_list_offs={:x}", offsets.ldr_list);

        let peb_ldr = mem.virt_read_addr_arch(arch, peb + offsets.peb_ldr)?;
        trace!("peb_ldr={:x}", peb_ldr);

        let module_base = mem.virt_read_addr_arch(arch, peb_ldr + offsets.ldr_list)?;

        Self::with_base(module_base, arch)
    }

    pub fn with_base(module_base: Address, arch: ArchitectureObj) -> Result<Win32ModuleListInfo> {
        trace!("module_base={:x}", module_base);

        let offsets = Win32ArchOffsets::from(arch);
        trace!("offsets={:?}", offsets);

        Ok(Win32ModuleListInfo {
            module_base,
            offsets,
        })
    }

    pub fn module_base(&self) -> Address {
        self.module_base
    }

    pub fn module_entry_list<P: Process>(
        &self,
        proc: &mut P,
        arch: ArchitectureObj,
    ) -> Result<Vec<Address>> {
        let mut out = vec![];
        self.module_entry_list_callback(proc, arch, (&mut out).into())?;
        Ok(out)
    }

    pub fn module_entry_list_callback<P: Process>(
        &self,
        proc: &mut P,
        arch: ArchitectureObj,
        mut callback: AddressCallback<P>,
    ) -> Result<()> {
        let list_start = self.module_base;
        let mut list_entry = list_start;
        for _ in 0..MAX_ITER_COUNT {
            if !callback.call(proc, list_entry) {
                break;
            }
            list_entry = proc.virt_mem().virt_read_addr_arch(arch, list_entry)?;
            // Break on misaligned entry. On NT 4.0 list end is misaligned, maybe it's a flag?
            if list_entry.is_null()
                || (list_entry.as_u64() & 0b111) != 0
                || list_entry == self.module_base
            {
                break;
            }
        }

        Ok(())
    }

    pub fn module_info_from_entry(
        &self,
        entry: Address,
        parent_eprocess: Address,
        mem: &mut impl VirtualMemory,
        arch: ArchitectureObj,
    ) -> Result<ModuleInfo> {
        let base = mem.virt_read_addr_arch(arch, entry + self.offsets.ldr_data_base)?;

        trace!("base={:x}", base);

        let size = mem
            .virt_read_addr_arch(arch, entry + self.offsets.ldr_data_size)?
            .as_usize();

        trace!("size={:x}", size);

        let path = mem.virt_read_unicode_string(arch, entry + self.offsets.ldr_data_full_name)?;
        trace!("path={}", path);

        let name = mem.virt_read_unicode_string(arch, entry + self.offsets.ldr_data_base_name)?;
        trace!("name={}", name);

        Ok(ModuleInfo {
            address: entry,
            parent_process: parent_eprocess,
            base,
            size,
            path: path.into(),
            name: name.into(),
            arch,
        })
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize))]
pub struct Win32ProcessInfo {
    pub base: ProcessInfo,

    // general information from eprocess
    pub dtb: Address,
    pub section_base: Address,
    pub exit_status: Win32ExitStatus,
    pub ethread: Address,
    pub wow64: Address,

    // teb
    pub teb: Option<Address>,
    pub teb_wow64: Option<Address>,

    // peb
    pub peb_native: Address,
    pub peb_wow64: Option<Address>,

    // modules
    pub module_info_native: Win32ModuleListInfo,
    pub module_info_wow64: Option<Win32ModuleListInfo>,
}

impl Win32ProcessInfo {
    pub fn wow64(&self) -> Address {
        self.wow64
    }

    pub fn peb(&self) -> Address {
        if let Some(peb) = self.peb_wow64 {
            peb
        } else {
            self.peb_native
        }
    }

    pub fn peb_native(&self) -> Address {
        self.peb_native
    }

    pub fn peb_wow64(&self) -> Option<Address> {
        self.peb_wow64
    }

    /// Return the module list information of process native architecture
    ///
    /// If the process is a wow64 process, module_info_wow64 is returned, otherwise, module_info_native is
    /// returned.
    pub fn module_info(&self) -> Win32ModuleListInfo {
        if !self.wow64.is_null() {
            self.module_info_wow64.unwrap()
        } else {
            self.module_info_native
        }
    }

    pub fn module_info_native(&self) -> Win32ModuleListInfo {
        self.module_info_native
    }

    pub fn module_info_wow64(&self) -> Option<Win32ModuleListInfo> {
        self.module_info_wow64
    }

    pub fn translator(&self) -> Win32VirtualTranslate {
        Win32VirtualTranslate::new(self.base.sys_arch, self.dtb)
    }
}

impl OsProcessInfo for Win32ProcessInfo {
    fn address(&self) -> Address {
        self.base.address
    }

    fn pid(&self) -> PID {
        self.base.pid
    }

    fn name(&self) -> String {
        self.base.name.as_ref().into()
    }

    fn sys_arch(&self) -> ArchitectureObj {
        self.base.sys_arch
    }

    fn proc_arch(&self) -> ArchitectureObj {
        self.base.proc_arch
    }
}

pub struct Win32Process<T> {
    pub virt_mem: T,
    pub proc_info: Win32ProcessInfo,
}

// TODO: can be removed i think
impl<T: Clone> Clone for Win32Process<T> {
    fn clone(&self) -> Self {
        Self {
            virt_mem: self.virt_mem.clone(),
            proc_info: self.proc_info.clone(),
        }
    }
}

impl<T: VirtualMemory> Process for Win32Process<T> {
    type VirtualMemoryType = T;
    //type VirtualTranslateType: VirtualTranslate;

    /// Retrieves virtual memory object for the process
    fn virt_mem(&mut self) -> &mut Self::VirtualMemoryType {
        &mut self.virt_mem
    }

    /// Retrieves virtual address translator for the process (if applicable)
    //fn vat(&mut self) -> Option<&mut Self::VirtualTranslateType>;

    /// Walks the process' module list and calls the provided callback for each module
    fn module_address_list_callback(
        &mut self,
        target_arch: Option<ArchitectureObj>,
        mut callback: ModuleAddressCallback<Self>,
    ) -> memflow::error::Result<()> {
        let infos = [
            (
                Some(self.proc_info.module_info_native),
                self.proc_info.base.sys_arch,
            ),
            (
                self.proc_info.module_info_wow64,
                self.proc_info.base.proc_arch,
            ),
        ];

        // Here we end up filtering out module_info_wow64 if it doesn't exist
        let iter = infos
            .iter()
            .filter(|(_, a)| {
                if let Some(ta) = target_arch {
                    *a == ta
                } else {
                    true
                }
            })
            .cloned()
            .filter_map(|(info, arch)| info.zip(Some(arch)));

        self.module_address_list_with_infos_callback(iter, &mut callback)
            .map_err(From::from)
    }

    /// Retreives a module by its structure address and architecture
    ///
    /// # Arguments
    /// * `address` - address where module's information resides in
    /// * `architecture` - architecture of the module. Should be either `ProcessInfo::proc_arch`, or `ProcessInfo::sys_arch`.
    fn module_info_by_address(
        &mut self,
        address: Address,
        architecture: ArchitectureObj,
    ) -> memflow::error::Result<ModuleInfo> {
        let info = if architecture == self.proc_info.sys_arch() {
            Some(&mut self.proc_info.module_info_native)
        } else if architecture == self.proc_info.proc_arch() {
            self.proc_info.module_info_wow64.as_mut()
        } else {
            None
        }
        .ok_or(Error::InvalidArchitecture)?;

        info.module_info_from_entry(
            address,
            self.proc_info.base.address,
            &mut self.virt_mem,
            architecture,
        )
        .map_err(From::from)
    }

    /// Retreives the process info
    fn info(&self) -> &ProcessInfo {
        &self.proc_info.base
    }
}

// TODO: replace the following impls with a dedicated builder
// TODO: add non cloneable thing
impl<'a, T: PhysicalMemory, V: VirtualTranslate>
    Win32Process<VirtualDMA<T, V, Win32VirtualTranslate>>
{
    pub fn with_kernel(kernel: Win32Kernel<T, V>, proc_info: Win32ProcessInfo) -> Self {
        let (phys_mem, vat) = kernel.virt_mem.destroy();
        let virt_mem = VirtualDMA::with_vat(
            phys_mem,
            proc_info.base.proc_arch,
            proc_info.translator(),
            vat,
        );

        Self {
            virt_mem,
            proc_info,
        }
    }

    /// Consume the self object and returns the containing memory connection
    pub fn destroy(self) -> (T, V) {
        self.virt_mem.destroy()
    }
}

impl<'a, T: PhysicalMemory, V: VirtualTranslate>
    Win32Process<VirtualDMA<&'a mut T, &'a mut V, Win32VirtualTranslate>>
{
    /// Constructs a new process by borrowing a kernel object.
    ///
    /// Internally this will create a `VirtualDMA` object that also
    /// borrows the PhysicalMemory and Vat objects from the kernel.
    ///
    /// The resulting process object is NOT cloneable due to the mutable borrowing.
    ///
    /// When u need a cloneable Process u have to use the `::with_kernel` function
    /// which will move the kernel object.
    pub fn with_kernel_ref(kernel: &'a mut Win32Kernel<T, V>, proc_info: Win32ProcessInfo) -> Self {
        let (phys_mem, vat) = kernel.virt_mem.borrow_both();
        let virt_mem = VirtualDMA::with_vat(
            phys_mem,
            proc_info.base.proc_arch,
            proc_info.translator(),
            vat,
        );

        Self {
            virt_mem,
            proc_info,
        }
    }
}

impl<T: VirtualMemory> Win32Process<T> {
    fn module_address_list_with_infos_callback(
        &mut self,
        module_infos: impl Iterator<Item = (Win32ModuleListInfo, ArchitectureObj)>,
        out: &mut ModuleAddressCallback<Self>,
    ) -> Result<()> {
        for (info, arch) in module_infos {
            let callback =
                &mut |s: &mut _, address| out.call(s, ModuleAddressInfo { address, arch });
            info.module_entry_list_callback(self, arch, callback.into())?;
        }
        Ok(())
    }

    pub fn module_entry_list(&mut self) -> Result<Vec<Address>> {
        let (info, arch) = if let Some(info_wow64) = self.proc_info.module_info_wow64 {
            (info_wow64, self.proc_info.base.proc_arch)
        } else {
            (
                self.proc_info.module_info_native,
                self.proc_info.base.sys_arch,
            )
        };

        info.module_entry_list(self, arch)
    }

    pub fn module_entry_list_native(&mut self) -> Result<Vec<Address>> {
        let (info, arch) = (
            self.proc_info.module_info_native,
            self.proc_info.base.sys_arch,
        );
        info.module_entry_list(self, arch)
    }

    pub fn module_entry_list_wow64(&mut self) -> Result<Vec<Address>> {
        let (info, arch) = (
            self.proc_info
                .module_info_wow64
                .ok_or(Error::Other("WoW64 module list does not exist"))?,
            self.proc_info.base.proc_arch,
        );
        info.module_entry_list(self, arch)
    }
}

impl<T: VirtualMemory> Win32Process<T>
where
    Self: Process,
{
    pub fn main_module_info(&mut self) -> Result<ModuleInfo> {
        let module_list = self.module_list()?;
        module_list
            .into_iter()
            .inspect(|module| trace!("{:x} {}", module.base, module.name))
            .find(|module| module.base == self.proc_info.section_base)
            .ok_or(Error::ModuleInfo)
    }

    pub fn module_info(&mut self, name: &str) -> Result<ModuleInfo> {
        let module_list = self.module_list()?;
        module_list
            .into_iter()
            .inspect(|module| trace!("{:x} {}", module.base, module.name))
            .find(|module| module.name.as_ref() == name)
            .ok_or(Error::ModuleInfo)
    }
}

impl<T> fmt::Debug for Win32Process<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.proc_info)
    }
}
