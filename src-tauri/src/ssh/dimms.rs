//! The physical memory modules behind `/proc/meminfo`'s totals.
//!
//! `/proc` knows how much RAM a host has and nothing whatever about the sticks it
//! is made of — module type, speed, manufacturer and part number are SMBIOS data,
//! a firmware table the kernel exposes verbatim at
//! `/sys/firmware/dmi/tables/DMI`. That file is mode 0400, so this is the one
//! part of the monitor that cannot work without root; the unprivileged fallback
//! is EDAC's sysfs, which is far poorer and only present on ECC hardware.
//!
//! The table is parsed here rather than shelled out to `dmidecode`, which is
//! absent from most minimal container and cloud images. The structure format is
//! `DSP0134` §6.1: a header of `type`/`length`/`handle`, a fixed-size formatted
//! area whose length grew with each SMBIOS version, then a set of NUL-separated
//! strings that the formatted area refers to by 1-based index. Reading a field
//! therefore means bounds-checking against *this* structure's `length` and not
//! against the layout of the newest spec.

use std::sync::Arc;

use serde::Serialize;

use super::client::Client;
use super::monitor::split_sections;
use super::{exec, SshError};

// `.replace("\r\n", "\n")` at the call site, for the same reason monitor.sh needs
// it: a CRLF checkout would ship `\r` to a remote POSIX shell.
const COLLECT_SCRIPT: &str = include_str!("dimms.sh");

/// The privileged half, as a *command line* — the collector's stdin is spent on
/// the script, and `sudo -S` wants stdin for the password. Kept free of
/// backslashes, `!` and newlines, the three things single-quoting does not
/// survive under fish and csh (see `tasks/lessons.md`).
pub const PRIVILEGED_COMMAND: &str =
    "echo @@dmi; od -An -v -tx1 /sys/firmware/dmi/tables/DMI 2>/dev/null; echo @@end";

#[derive(Debug, Clone, Default, Serialize)]
pub struct MemoryModule {
    /// The silkscreen name of the slot, e.g. `DIMM_A1` — what you read off the
    /// board when deciding which stick to pull.
    pub locator: String,
    pub bank: Option<String>,
    /// `DDR4`, `DDR5`, `LPDDR5`… `Unknown` on a hypervisor that fakes the table.
    pub kind: String,
    pub size_bytes: u64,
    /// Rated speed in MT/s (what the module is capable of).
    pub speed_mts: Option<u32>,
    /// Speed it is actually clocked at — lower than rated whenever the board
    /// down-clocks for a fully populated bank, which is a real thing to notice.
    pub configured_mts: Option<u32>,
    pub manufacturer: Option<String>,
    pub part_number: Option<String>,
    pub rank: Option<u8>,
    pub form_factor: Option<String>,
    /// Only the traits worth showing: `Registered`, `Unbuffered`, `LRDIMM`,
    /// `Non-volatile`. `Synchronous` is set on every module made this century.
    pub detail: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct MemoryInventory {
    pub modules: Vec<MemoryModule>,
    /// Slots the firmware reports, populated or not.
    pub total_slots: usize,
    pub empty_slots: usize,
    pub ecc: Option<String>,
    pub max_capacity_bytes: Option<u64>,
    /// `dmi` or `edac` — the two have very different richness, and the UI says
    /// which one it is rather than letting `edac`'s missing fields read as a
    /// host that has no part numbers.
    pub source: Option<&'static str>,
    pub warnings: Vec<String>,
}

/// Both sources as collected, before deciding which one to believe.
#[derive(Debug, Clone, Default)]
pub struct RawModules {
    pub edac: Vec<MemoryModule>,
    /// The raw SMBIOS structure table. Empty when the read was refused.
    pub dmi: Vec<u8>,
}

impl RawModules {
    /// True when neither source produced anything — the only case worth paying a
    /// sudo escalation for.
    pub fn is_empty(&self) -> bool {
        self.edac.is_empty() && self.dmi.is_empty()
    }
}

pub async fn collect(ssh: &Arc<russh::client::Handle<Client>>) -> Result<RawModules, SshError> {
    let script = COLLECT_SCRIPT.replace("\r\n", "\n");
    let output = exec::run(ssh, "sh", Some(&script)).await?;
    parse(&output.stdout).map_err(SshError::Monitor)
}

pub fn parse(stdout: &str) -> Result<RawModules, String> {
    let sections = split_sections(stdout);
    if !sections.contains_key("end") {
        return Err(if sections.is_empty() {
            "the host produced no output; its login shell may run a forced command".into()
        } else {
            "the memory-module read was cut short before it finished".into()
        });
    }
    Ok(RawModules {
        edac: parse_edac(sections.get("edac").copied().unwrap_or("")),
        dmi: decode_hex(sections.get("dmi").copied().unwrap_or("")),
    })
}

/// Overlays an elevated re-read of the DMI table. Returns false if the output was
/// cut short, in which case the caller keeps whatever the unprivileged pass found.
pub fn merge_privileged(raw: &mut RawModules, stdout: &str) -> bool {
    let sections = split_sections(stdout);
    if !sections.contains_key("end") {
        return false;
    }
    if let Some(text) = sections.get("dmi") {
        raw.dmi = decode_hex(text);
    }
    true
}

/// DMI wins whenever it produced a table: EDAC knows a module's type and size and
/// nothing else, so falling back to it when the richer source answered would throw
/// away every part number.
pub fn inventory(raw: &RawModules) -> MemoryInventory {
    let dmi = from_dmi(&raw.dmi);
    if dmi.source.is_some() {
        return dmi;
    }

    let (modules, empty): (Vec<_>, Vec<_>) =
        raw.edac.iter().cloned().partition(|module| module.size_bytes > 0);
    if modules.is_empty() {
        return MemoryInventory::default();
    }
    MemoryInventory {
        total_slots: modules.len() + empty.len(),
        empty_slots: empty.len(),
        modules,
        source: Some("edac"),
        ..Default::default()
    }
}

// ------------------------------------------------------------------- SMBIOS

fn from_dmi(bytes: &[u8]) -> MemoryInventory {
    let mut inventory = MemoryInventory::default();
    if bytes.is_empty() {
        return inventory;
    }

    for structure in structures(bytes) {
        match structure.kind {
            // Physical Memory Array. A two-socket machine has one per socket, so
            // the capacities add up; the correction type is the same on both and
            // the first one to report it stands.
            16 => {
                if inventory.ecc.is_none() {
                    inventory.ecc = structure.byte(0x06).and_then(error_correction).map(str::to_string);
                }
                // Maximum Capacity is a DWORD of *kilobytes*, with 0x8000_0000
                // meaning "see the 2.7+ extended QWORD of bytes instead".
                let capacity = match structure.dword(0x07) {
                    Some(0x8000_0000) | None => structure.qword(0x0F),
                    Some(0) | Some(0xFFFF_FFFF) => None,
                    Some(kb) => Some(u64::from(kb) * 1024),
                };
                if let Some(capacity) = capacity {
                    *inventory.max_capacity_bytes.get_or_insert(0) += capacity;
                }
            }
            17 => {
                inventory.total_slots += 1;
                match memory_device(&structure) {
                    Some(module) => inventory.modules.push(module),
                    None => inventory.empty_slots += 1,
                }
            }
            _ => {}
        }
    }

    // Slots, not modules: a table that lists four empty sockets did answer the
    // question, and must not send the caller off to escalate for a second look.
    if inventory.total_slots > 0 {
        inventory.source = Some("dmi");
    }
    inventory
}

/// One Memory Device (type 17), or `None` for an unpopulated slot.
fn memory_device(structure: &Structure) -> Option<MemoryModule> {
    let size_bytes = match structure.word(0x0C)? {
        // Size is the field that says whether anything is plugged in at all.
        0 => return None,
        0xFFFF => 0,
        // 0x7FFF is the escape hatch for modules of 32 GB and over, which do not
        // fit the 15-bit megabyte field: the real value is the 2.7+ extended
        // DWORD, in megabytes, with the top bit reserved.
        0x7FFF => u64::from(structure.dword(0x1C).unwrap_or(0) & 0x7FFF_FFFF) * 1024 * 1024,
        // Bit 15 clear means the value is in megabytes, set means kilobytes.
        raw if raw & 0x8000 != 0 => u64::from(raw & 0x7FFF) * 1024,
        raw => u64::from(raw) * 1024 * 1024,
    };

    let detail = structure.word(0x13).map(type_detail).unwrap_or_default();
    Some(MemoryModule {
        locator: structure
            .string(0x10)
            .unwrap_or_else(|| format!("slot {:#06x}", structure.handle())),
        bank: structure.string(0x11),
        kind: structure.byte(0x12).map(memory_type).unwrap_or("Unknown").to_string(),
        size_bytes,
        // 0xFFFF on a speed field means "too fast for a 16-bit MT/s value", and
        // the true figure is in the SMBIOS 3.3 extended DWORDs.
        speed_mts: speed(structure, 0x15, 0x54),
        configured_mts: speed(structure, 0x20, 0x58),
        manufacturer: structure.string(0x17),
        // Serial number (0x18) and asset tag (0x19) are deliberately not read:
        // they identify the machine and answer nothing anyone opens this panel for.
        part_number: structure.string(0x1A),
        // Low nibble of Attributes; 0 means the firmware did not say.
        rank: structure.byte(0x1B).map(|attributes| attributes & 0x0F).filter(|rank| *rank > 0),
        form_factor: structure.byte(0x0E).and_then(form_factor).map(str::to_string),
        detail,
    })
}

fn speed(structure: &Structure, offset: usize, extended: usize) -> Option<u32> {
    match structure.word(offset)? {
        0 => None,
        0xFFFF => structure.dword(extended).filter(|value| *value > 0),
        value => Some(u32::from(value)),
    }
}

struct Structure<'a> {
    kind: u8,
    /// The formatted area, starting at the type byte. Never longer than the
    /// structure's own declared length, which is what bounds every field read.
    data: &'a [u8],
    strings: Vec<String>,
}

impl Structure<'_> {
    fn handle(&self) -> u16 {
        self.word(0x02).unwrap_or(0)
    }

    fn byte(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    fn word(&self, offset: usize) -> Option<u16> {
        let bytes = self.data.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    fn dword(&self, offset: usize) -> Option<u32> {
        let bytes = self.data.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    fn qword(&self, offset: usize) -> Option<u64> {
        let bytes = self.data.get(offset..offset + 8)?;
        Some(u64::from_le_bytes(bytes.try_into().ok()?))
    }

    /// A string field holds a 1-based index into the structure's string set; 0
    /// means "not specified". Firmware also loves to fill these with placeholder
    /// text and trailing spaces, which is worth nobody's screen space.
    fn string(&self, offset: usize) -> Option<String> {
        let index = self.byte(offset)?;
        let value = self.strings.get(usize::from(index).checked_sub(1)?)?.trim();
        let placeholder = [
            "unknown",
            "not specified",
            "none",
            "n/a",
            "no module installed",
            "not available",
            "to be filled by o.e.m.",
            "empty",
            "default string",
        ];
        if value.is_empty() || placeholder.iter().any(|text| value.eq_ignore_ascii_case(text)) {
            return None;
        }
        Some(value.to_string())
    }
}

/// Walks the structure table. Stops at the end-of-table structure, at a length
/// that cannot be right, or at a string set the buffer ends in the middle of —
/// a truncated table yields the structures that were whole, not an error, because
/// the memory devices sit early in it.
fn structures(bytes: &[u8]) -> Vec<Structure<'_>> {
    let mut out = Vec::new();
    let mut cursor = 0usize;

    while cursor + 4 <= bytes.len() {
        let kind = bytes[cursor];
        let length = usize::from(bytes[cursor + 1]);
        if length < 4 || cursor + length > bytes.len() {
            break;
        }
        let data = &bytes[cursor..cursor + length];

        // The string set runs to a double NUL. A structure with no strings at all
        // is still terminated by two, so that pair is not a zero-length string.
        let mut position = cursor + length;
        let mut strings = Vec::new();
        if bytes.get(position) == Some(&0) && bytes.get(position + 1) == Some(&0) {
            position += 2;
        } else {
            loop {
                let start = position;
                while position < bytes.len() && bytes[position] != 0 {
                    position += 1;
                }
                if position >= bytes.len() {
                    return out;
                }
                strings.push(String::from_utf8_lossy(&bytes[start..position]).into_owned());
                position += 1;
                match bytes.get(position) {
                    Some(0) => {
                        position += 1;
                        break;
                    }
                    Some(_) => continue,
                    None => return out,
                }
            }
        }

        out.push(Structure { kind, data, strings });
        if kind == 127 {
            break;
        }
        cursor = position;
    }
    out
}

/// `od -An -v -tx1` output: whitespace-separated pairs of hex digits, nothing
/// else. Anything that is not one ends the decode rather than being skipped — a
/// permission-denied message landing in the middle of the stream must not be
/// stitched over to produce a table that parses into plausible nonsense.
fn decode_hex(text: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    for token in text.split_whitespace() {
        match u8::from_str_radix(token, 16) {
            Ok(byte) if token.len() == 2 => bytes.push(byte),
            _ => break,
        }
    }
    bytes
}

// DSP0134 7.18.2. Only the families a running machine can actually have are
// spelled out; the rest fall through to the numeric form, which is still more
// useful than "Unknown" when a new generation ships before this list is updated.
fn memory_type(value: u8) -> &'static str {
    match value {
        0x03 => "DRAM",
        0x07 => "RAM",
        0x0F => "SDRAM",
        0x11 => "RDRAM",
        0x12 => "DDR",
        0x13 => "DDR2",
        0x14 => "DDR2 FB-DIMM",
        0x18 => "DDR3",
        0x19 => "FBD2",
        0x1A => "DDR4",
        0x1B => "LPDDR",
        0x1C => "LPDDR2",
        0x1D => "LPDDR3",
        0x1E => "LPDDR4",
        0x1F => "Non-volatile",
        0x20 => "HBM",
        0x21 => "HBM2",
        0x22 => "DDR5",
        0x23 => "LPDDR5",
        0x24 => "HBM3",
        _ => "Unknown",
    }
}

// DSP0134 7.18.1. `Unknown` and `Other` are dropped rather than shown: the point
// of the field is to tell a SODIMM from a DIMM.
fn form_factor(value: u8) -> Option<&'static str> {
    Some(match value {
        0x03 => "SIMM",
        0x05 => "Chip",
        0x09 => "DIMM",
        0x0A => "TSOP",
        0x0B => "Row of chips",
        0x0C => "RIMM",
        0x0D => "SODIMM",
        0x0E => "SRIMM",
        0x0F => "FB-DIMM",
        0x10 => "Die",
        _ => return None,
    })
}

// DSP0134 7.18.3, a bit field. Everything below bit 12 describes DRAM
// technologies that stopped shipping decades ago, or is `Synchronous`, which is
// set on every module in every machine this will ever see.
fn type_detail(value: u16) -> Vec<String> {
    [(12, "Non-volatile"), (13, "Registered"), (14, "Unbuffered"), (15, "LRDIMM")]
        .into_iter()
        .filter(|(bit, _)| value & (1 << bit) != 0)
        .map(|(_, name)| name.to_string())
        .collect()
}

// DSP0134 7.17.3.
fn error_correction(value: u8) -> Option<&'static str> {
    Some(match value {
        0x03 => "None",
        0x04 => "Parity",
        0x05 => "Single-bit ECC",
        0x06 => "Multi-bit ECC",
        0x07 => "CRC",
        _ => return None,
    })
}

// --------------------------------------------------------------------- EDAC

/// `slot <path>` then `field value` lines, as emitted by dimms.sh.
fn parse_edac(text: &str) -> Vec<MemoryModule> {
    let mut modules: Vec<MemoryModule> = Vec::new();

    for line in text.lines() {
        let (key, value) = match line.trim_end().split_once(' ') {
            Some((key, value)) => (key, value.trim()),
            None => continue,
        };
        if key == "slot" {
            modules.push(MemoryModule {
                // Overwritten by dimm_label when the firmware supplied one; the
                // directory name (`dimm3`) is a poor label but better than blank.
                locator: value.rsplit('/').next().unwrap_or(value).to_string(),
                kind: "Unknown".into(),
                ..Default::default()
            });
            continue;
        }
        let Some(module) = modules.last_mut() else { continue };
        match key {
            "dimm_label" if !value.is_empty() => module.locator = value.to_string(),
            "dimm_location" if !value.is_empty() => module.bank = Some(value.to_string()),
            // `size` is in megabytes. An unpopulated slot reads 0, which
            // `inventory` counts as empty rather than as a 0-byte module.
            "size" => module.size_bytes = value.parse::<u64>().unwrap_or(0) * 1024 * 1024,
            // EDAC writes the buffering into the type itself
            // (`Unbuffered-DDR4`), where the rest of this module keeps the two
            // apart. Split it so both sources produce the same shape.
            "dimm_mem_type" if !value.is_empty() => match value.split_once('-') {
                Some((buffering, kind)) => {
                    module.kind = kind.to_string();
                    module.detail = vec![buffering.to_string()];
                }
                None => module.kind = value.to_string(),
            },
            _ => {}
        }
    }
    modules
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real SMBIOS 3.6 table, dumped from a running machine through
    // `MSSmBios_RawSMBiosTables` and rendered in `od -An -v -tx1` form, i.e. byte
    // for byte what the collector sends back. Serial numbers and asset tags were
    // overwritten with `X` of the same length, so every offset in the table is
    // still the one the firmware wrote.
    //
    // Ground truth, from the same machine: 4 × 16 GB Kingston DDR5-5600 in
    // Controller0-DIMMA1/A2 and Controller1-DIMMB1/B2, non-ECC, 128 GB maximum.
    const REAL_TABLE: &str = include_str!("testdata/dmi-ddr5.txt");

    fn real_inventory() -> MemoryInventory {
        let raw = parse(&format!("@@edac\n@@dmi\n{REAL_TABLE}@@end\n")).expect("sentinel present");
        inventory(&raw)
    }

    #[test]
    fn reads_a_real_smbios_table() {
        let inventory = real_inventory();
        assert_eq!(inventory.source, Some("dmi"));
        assert_eq!(inventory.total_slots, 4);
        assert_eq!(inventory.empty_slots, 0);
        assert_eq!(inventory.ecc.as_deref(), Some("None"));
        assert_eq!(inventory.max_capacity_bytes, Some(128 * 1024 * 1024 * 1024));

        let first = &inventory.modules[0];
        assert_eq!(first.locator, "Controller0-DIMMA1");
        assert_eq!(first.kind, "DDR5");
        assert_eq!(first.size_bytes, 16 * 1024 * 1024 * 1024);
        assert_eq!(first.speed_mts, Some(5600));
        assert_eq!(first.configured_mts, Some(5600));
        assert_eq!(first.manufacturer.as_deref(), Some("Kingston"));
        // The firmware pads the part number out with spaces to a fixed width.
        assert_eq!(first.part_number.as_deref(), Some("KF564C32-16"));
        assert_eq!(first.rank, Some(1));
        assert_eq!(first.form_factor.as_deref(), Some("DIMM"));
        assert_eq!(first.bank.as_deref(), Some("BANK 0"));
        // Only bit 7 (Synchronous) is set on these, and that is not worth showing.
        assert!(first.detail.is_empty(), "{:?}", first.detail);

        let locators: Vec<&str> = inventory.modules.iter().map(|m| m.locator.as_str()).collect();
        assert_eq!(
            locators,
            ["Controller0-DIMMA1", "Controller0-DIMMA2", "Controller1-DIMMB1", "Controller1-DIMMB2"]
        );
    }

    // The serial number and asset tag sit between the manufacturer and the part
    // number in the string set, so reading the part number at all proves the
    // 1-based index walk is right — and this asserts the two identifying fields
    // never reach the struct.
    #[test]
    fn carries_no_serial_number() {
        let inventory = real_inventory();
        let rendered = format!("{:?}", inventory.modules);
        assert!(!rendered.contains('X'), "{rendered}");
    }

    /// Rebuilds the table, handing each structure's formatted area to `edit`.
    /// Patching in place is not enough for anything that changes a length: the
    /// string set follows the formatted area, so shortening a structure without
    /// moving the bytes after it leaves the parser reading padding as a string.
    fn rebuild(bytes: &[u8], mut edit: impl FnMut(u8, &mut Vec<u8>)) -> Vec<u8> {
        let mut out = Vec::new();
        for structure in structures(bytes) {
            let mut data = structure.data.to_vec();
            edit(structure.kind, &mut data);
            data[1] = data.len() as u8;
            out.extend_from_slice(&data);
            for text in &structure.strings {
                out.extend_from_slice(text.as_bytes());
                out.push(0);
            }
            // A structure with no strings still ends in two NULs, not one.
            if structure.strings.is_empty() {
                out.push(0);
            }
            out.push(0);
        }
        out
    }

    // Everything about a structure's size comes from its own `length` byte, and a
    // pre-2.7 table simply stops before the later fields exist. Cutting the real
    // modules back to SMBIOS 2.3 is the cheapest honest way to prove no read runs
    // off the end of one.
    #[test]
    fn an_old_smbios_version_loses_fields_instead_of_bytes() {
        let bytes = rebuild(&decode_hex(REAL_TABLE), |kind, data| {
            if kind == 17 {
                data.truncate(0x1B); // SMBIOS 2.3: ends after the part number
            }
        });

        let inventory = from_dmi(&bytes);
        assert_eq!(inventory.modules.len(), 4);
        let first = &inventory.modules[0];
        assert_eq!(first.kind, "DDR5");
        assert_eq!(first.speed_mts, Some(5600));
        assert_eq!(first.part_number.as_deref(), Some("KF564C32-16"));
        // Attributes and configured speed are past the shortened length.
        assert_eq!(first.rank, None);
        assert_eq!(first.configured_mts, None);
    }

    // 32 GB and larger will not fit the 15-bit megabyte field, so the firmware
    // parks 0x7FFF there and puts the real figure in the extended DWORD. Getting
    // this wrong reports 32 TB, or 32 MB, rather than failing.
    #[test]
    fn a_large_module_comes_from_the_extended_size_field() {
        let bytes = rebuild(&decode_hex(REAL_TABLE), |kind, data| {
            if kind == 17 {
                data[0x0C..0x0E].copy_from_slice(&0x7FFFu16.to_le_bytes());
                data[0x1C..0x20].copy_from_slice(&(64u32 * 1024).to_le_bytes());
            }
        });

        let inventory = from_dmi(&bytes);
        assert_eq!(inventory.modules.len(), 4);
        assert_eq!(inventory.modules[0].size_bytes, 64 * 1024 * 1024 * 1024);
    }

    // A slot with nothing in it is size 0, and it is still a slot. Listing it as a
    // module would put a 0 GB stick on screen.
    #[test]
    fn an_empty_slot_is_counted_but_not_listed() {
        let mut emptied = 0;
        let bytes = rebuild(&decode_hex(REAL_TABLE), |kind, data| {
            if kind == 17 && emptied < 2 {
                data[0x0C..0x0E].copy_from_slice(&0u16.to_le_bytes());
                emptied += 1;
            }
        });

        let inventory = from_dmi(&bytes);
        assert_eq!(inventory.total_slots, 4);
        assert_eq!(inventory.empty_slots, 2);
        assert_eq!(inventory.modules.len(), 2);
    }

    // The permission-denied case: the file is 0400, so an unprivileged `od` prints
    // nothing at all and the caller must be able to tell that from a bad read.
    #[test]
    fn an_unreadable_table_yields_nothing_to_escalate_from() {
        let raw = parse("@@edac\n@@dmi\n@@end\n").expect("sentinel present");
        assert!(raw.is_empty());
        assert_eq!(inventory(&raw).source, None);
    }

    #[test]
    fn a_truncated_read_is_an_error_not_an_empty_host() {
        let table = format!("@@edac\n@@dmi\n{}", &REAL_TABLE[..200]);
        assert!(parse(&table).is_err());
        assert!(parse("").is_err());
    }

    // od never emits anything but hex pairs, so a token that is not one means the
    // stream carries something else — stopping keeps a stray `sudo:` line from
    // being stitched into the middle of a table.
    #[test]
    fn hex_decoding_stops_at_the_first_thing_that_is_not_a_byte() {
        assert_eq!(decode_hex(" 00 7f ff\n 10 20"), [0x00, 0x7F, 0xFF, 0x10, 0x20]);
        assert_eq!(decode_hex("00 01 sudo: a password is required 02"), [0x00, 0x01]);
        // `7` alone is valid hex but half a byte, and would shift everything after it.
        assert_eq!(decode_hex("00 7 01"), [0x00]);
    }

    // The unprivileged fallback. Real `dimm_mem_type` text, and the buffering
    // prefix EDAC folds into it.
    #[test]
    fn reads_the_edac_fallback() {
        let raw = parse(concat!(
            "@@edac\n",
            "slot /sys/devices/system/edac/mc/mc0/dimm0\n",
            "dimm_label CPU_SrcID#0_MC#0_Chan#0_DIMM#0\n",
            "dimm_location channel 0 slot 0\n",
            "dimm_mem_type Registered-DDR4\n",
            "size 32768\n",
            "slot /sys/devices/system/edac/mc/mc0/dimm1\n",
            "dimm_label \n",
            "dimm_mem_type Unknown\n",
            "size 0\n",
            "@@dmi\n@@end\n",
        ))
        .expect("sentinel present");

        let inventory = inventory(&raw);
        assert_eq!(inventory.source, Some("edac"));
        assert_eq!(inventory.total_slots, 2);
        assert_eq!(inventory.empty_slots, 1);
        assert_eq!(inventory.modules.len(), 1);

        let module = &inventory.modules[0];
        assert_eq!(module.locator, "CPU_SrcID#0_MC#0_Chan#0_DIMM#0");
        assert_eq!(module.kind, "DDR4");
        assert_eq!(module.detail, ["Registered"]);
        assert_eq!(module.size_bytes, 32 * 1024 * 1024 * 1024);
        assert_eq!(module.bank.as_deref(), Some("channel 0 slot 0"));
        assert_eq!(module.part_number, None);
    }

    // DMI is the richer source; a host with both must not be reported from EDAC.
    #[test]
    fn dmi_outranks_edac_when_both_answer() {
        let raw = parse(&format!(
            "@@edac\nslot /sys/devices/system/edac/mc/mc0/dimm0\ndimm_mem_type DDR3\nsize 4096\n@@dmi\n{REAL_TABLE}@@end\n"
        ))
        .expect("sentinel present");
        let inventory = inventory(&raw);
        assert_eq!(inventory.source, Some("dmi"));
        assert_eq!(inventory.modules[0].kind, "DDR5");
    }

    // The privileged fragment travels as a command line under the account's login
    // shell, where single quotes do not protect these three (see lessons.md).
    #[test]
    fn the_privileged_fragment_survives_fish_and_csh() {
        assert!(!PRIVILEGED_COMMAND.contains('\\'));
        assert!(!PRIVILEGED_COMMAND.contains('!'));
        assert!(!PRIVILEGED_COMMAND.contains('\n'));
    }

    // The script is piped to a remote POSIX sh; a CRLF checkout would break every
    // line of it. `.gitattributes` pins it, and the collector strips it anyway.
    #[test]
    fn the_collector_script_has_no_carriage_returns_after_normalising() {
        assert!(!COLLECT_SCRIPT.replace("\r\n", "\n").contains('\r'));
        assert!(COLLECT_SCRIPT.contains("@@end"));
    }
}
