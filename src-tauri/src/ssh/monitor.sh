# SshManager remote monitor collector.
#
# Sent to a bare `sh` on the exec channel's STDIN, never as `sh -c '<this>'`:
# the account's login shell may be fish (which honours \' and \ inside single
# quotes) or csh (which history-expands `!` inside them), and the awk below
# contains both. As channel data these bytes reach a real POSIX shell untouched.
#
# Deliberately free of `set -e` — one unreadable file must cost one section, not
# the whole sample. Every section is independent, and the reader keys on the
# `@@name` markers and the final `@@end` sentinel rather than on the exit status
# (`cat` exits nonzero whenever a pid vanished mid-read, i.e. most polls).
export LC_ALL=C

echo "@@uname"
uname -s -r -m 2>/dev/null

echo "@@conf"
echo "clk_tck $(getconf CLK_TCK 2>/dev/null)"
echo "pagesize $(getconf PAGESIZE 2>/dev/null)"
echo "now $(date +%s 2>/dev/null)"

# --- Counters whose deltas become rates. Read close together and before the
# --- expensive process walk, so they share as near one instant as possible.
echo "@@stat"
cat /proc/stat 2>/dev/null

echo "@@uptime"
cat /proc/uptime 2>/dev/null

echo "@@netdev"
cat /proc/net/dev 2>/dev/null

echo "@@diskstats"
cat /proc/diskstats 2>/dev/null

# --- Process table. `cat | awk`, never `awk /proc/[0-9]*/stat`: a pid exiting
# --- between glob expansion and awk's open is a FATAL error in gawk/mawk/busybox
# --- awk, which abandons the remaining operands and leaves output that looks
# --- perfectly well-formed but is truncated. `cat` reports and carries on.
echo "@@procs"
cat /proc/[0-9]*/stat 2>/dev/null | awk '
  {
    # Field 2 (comm) is parenthesised and may contain spaces and parentheses, so
    # $14-style indexing shifts and indexing from NF is wrong too (the field
    # count grew across kernel versions). Split on the LAST ")" instead.
    tail = match($0, /\)[^)]*$/)
    if (tail == 0) next
    open = index($0, "(")
    if (open == 0 || tail <= open) next
    comm = substr($0, open + 1, tail - open - 1)
    n = split(substr($0, tail + 1), f, " ")
    if (n < 22) next
    gsub(/[ \t]/, "_", comm)
    # Offsets into the post-")" remainder (proc(5) field number = index + 2):
    # state 1, ppid 2, utime 12, stime 13, num_threads 18, starttime 20,
    # rss 22 (in PAGES, not kB — scaled by pagesize above).
    print $1, f[1], f[2], f[12], f[13], f[18], f[20], f[22], comm
  }' 2>/dev/null

# `user:32=` because a bare `user=` truncates to 8 characters with a trailing
# `+`; `args:200=` caps the pathological JVM command line. Fallbacks cover
# busybox ps, which rejects the width syntax.
echo "@@ps"
ps -e -o pid=,user:32=,args:200= 2>/dev/null ||
  ps -e -o pid=,user=,args= 2>/dev/null ||
  ps -o pid=,user=,args= 2>/dev/null

echo "@@meminfo"
cat /proc/meminfo 2>/dev/null

echo "@@loadavg"
cat /proc/loadavg 2>/dev/null

# `model name` is absent on aarch64/riscv; the device-tree model is the usual
# fallback there. First non-empty line wins.
echo "@@cpuinfo"
awk -F': *' '
  /^model name/ || /^Model[ \t]*:/ || /^Hardware/ || /^cpu model/ { print $2; exit }
' /proc/cpuinfo 2>/dev/null
cat /sys/firmware/devicetree/base/model 2>/dev/null | tr -d '\000'
echo ""

# Pressure stall information (kernel 4.20+). A far better "is this box in
# trouble" signal than load average; absent on older kernels, which is fine.
echo "@@pressure"
for res in cpu io memory; do
  echo "$res $(awk '/^some/ { print $2; exit }' /proc/pressure/$res 2>/dev/null)"
done

# `-l` keeps a dead NFS/CIFS mount from blocking the whole sample; busybox df
# has no `-l`, hence the fallback.
echo "@@df"
df -P -k -l 2>/dev/null || df -P -k 2>/dev/null

# Mount point, type and options only. Options carry `hidepid=`, which is the
# free way to detect a process list that will come back near-empty.
echo "@@mounts"
awk '{ print $2, $3, $4 }' /proc/mounts 2>/dev/null

# Inside a cgroup-limited container /proc reports the HOST's RAM and core count.
# These turn that from a plausible wrong number into a labelled one.
echo "@@cgroup"
echo "mem_max $(cat /sys/fs/cgroup/memory.max 2>/dev/null)"
echo "cpu_max $(cat /sys/fs/cgroup/cpu.max 2>/dev/null)"
echo "mem_limit_v1 $(cat /sys/fs/cgroup/memory/memory.limit_in_bytes 2>/dev/null)"
echo "cpu_quota_v1 $(cat /sys/fs/cgroup/cpu/cpu.cfs_quota_us 2>/dev/null)"
echo "cpu_period_v1 $(cat /sys/fs/cgroup/cpu/cpu.cfs_period_us 2>/dev/null)"

# Whole devices only. /proc/diskstats lists sda, sda1 and dm-0 alike, so summing
# it double- or triple-counts; /sys/block is exactly the set of real devices.
echo "@@blockdevs"
ls /sys/block 2>/dev/null

# Per-process network. There is no per-process byte counter in /proc -- and
# /proc/<pid>/net/dev is a trap, being per network *namespace*, so every process
# in the root namespace reports identical host-wide totals. The real path is
# per-socket: `-i` carries tcp_info's cumulative bytes_sent/bytes_received, `-e`
# the socket inode to diff them by across polls, and `-p` the owning process.
#
# `state connected` drops listeners and TIME_WAIT, neither of which carries bytes,
# which on a busy server is most of the table. Attribution here only covers the
# login user's own sockets; the caller re-runs this under sudo when asked.
echo "@@sockets"
ss -H -tunaep -i state connected 2>/dev/null | head -n 6000

# The executable behind each pid. argv is whatever the process chose to claim, but
# this symlink is the kernel's own record -- and it still resolves after the file
# is unlinked, where Linux appends " (deleted)". One `ls` covers every pid on the
# host in about a millisecond. Unprivileged it resolves only the login user's own
# processes; the target of someone else's is EACCES, and those lines have no
# " -> " to match.
#
# Left as raw `ls -l` rather than projected here, so that this and the elevated
# re-run (which cannot pipe through awk without putting a backslash-laden program
# on a sudo command line) produce one format for one parser.
echo "@@exe"
ls -l /proc/[0-9]*/exe 2>/dev/null

echo "@@end"
