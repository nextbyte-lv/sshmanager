# SshManager physical-memory-module collector.
#
# One shot on demand, deliberately not part of the 2-second sample: DIMMs do not
# change while a host is up, and the DMI half of this needs root, which sampling
# must never ask for.
#
# Sent to a bare `sh` on the exec channel's STDIN for the same reason monitor.sh
# is — see the header there. Same contract too: independent sections, a final
# `@@end` sentinel, and no `set -e`.
export LC_ALL=C

# EDAC's sysfs view of the memory controller. The only source here a normal user
# can read — but it exists only where an EDAC driver bound to the memory
# controller, which in practice means ECC-capable server and workstation
# hardware, never a VM and rarely a consumer board. Carries type and size only:
# no manufacturer, part number or speed.
echo "@@edac"
for slot in /sys/devices/system/edac/mc/mc*/dimm* /sys/devices/system/edac/mc/mc*/rank*; do
  [ -d "$slot" ] || continue
  echo "slot $slot"
  for field in dimm_label dimm_location dimm_mem_type dimm_dev_type size; do
    [ -r "$slot/$field" ] || continue
    echo "$field $(cat "$slot/$field" 2>/dev/null)"
  done
done

# The SMBIOS structure table — the same bytes `dmidecode` reads, parsed in Rust
# so this works on the many hosts that do not have dmidecode installed. The file
# is mode 0400, so unprivileged this prints nothing and the caller re-runs just
# the `od` under sudo.
#
# `od -An -v -tx1` rather than base64: POSIX, present in busybox, and needs no
# decoder crate for 5 kB of output. `-v` is not optional — without it od collapses
# runs of identical lines to a single `*`, and a memory table is mostly padding.
echo "@@dmi"
od -An -v -tx1 /sys/firmware/dmi/tables/DMI 2>/dev/null

echo "@@end"
