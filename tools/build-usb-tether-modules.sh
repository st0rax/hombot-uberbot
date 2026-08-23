#!/usr/bin/env bash
set -euo pipefail

ROOT="$(pwd)"
BUILD="$RUNNER_TEMP/hombot-kernel-build"
OUT="$ROOT/out/usb-tether-modules"
CROSS=arm-linux-gnueabi-

rm -rf "$BUILD" "$OUT"
mkdir -p "$BUILD" "$OUT"

git clone --depth 1 https://github.com/larixer/kernel.rk.git "$BUILD/kernel.rk"
cd "$BUILD/kernel.rk"
./make_kernel.sh

cd kernel-2.6.33
cp ../files/arch/arm/configs/rk_hit_v2_ubif_defconfig .config
patch -p1 < "$ROOT/tools/kernel-modern-toolchain.patch"

# New ARM binutils treat `#` as a comment marker and reject the legacy section
# flag spelling used by 2.6.33.  Convert it to the equivalent ELF spelling.
# 2.6.33 spells these with and without spaces, and .piggydata carries only the
# alloc flag, so cover every form and then assert none survived.
grep -rlE ',[[:space:]]*#alloc' arch/arm | while IFS= read -r source; do
  sed -i -E     -e 's|,[[:space:]]*#alloc[[:space:]]*,[[:space:]]*#execinstr|, "ax", %progbits|g'     -e 's|,[[:space:]]*#alloc[[:space:]]*,[[:space:]]*#write|, "aw", %progbits|g'     -e 's|,[[:space:]]*#alloc[[:space:]]*$|, "a", %progbits|g'     "$source"
done
! grep -rq '#alloc' arch/arm

# Perl 5.22 removed `defined(@array)`, which kernel/timeconst.pl still uses to
# test whether a canned HZ table exists.  The bare array already has the right
# truthiness, so drop the defined() exactly as upstream did.
grep -q 'if (!defined(@val))' kernel/timeconst.pl
sed -i 's|if (!defined(@val))|if (!@val)|' kernel/timeconst.pl
perl -c kernel/timeconst.pl

# GCC 4.3 honoured `register const ... asm("r2")` even when the initialiser was
# a compile-time constant.  Current GCC folds such a variable into a constant and
# is free to keep it in any register, which trips the kernel's own __asmeq()
# register check -- e.g. `put_user(0, tsk->clear_child_tid)` in kernel/fork.c
# compiled to `.ifnc r3,r2 ; .err`.  Upstream dropped the `const` on this operand
# for exactly this reason (see arch/arm/include/asm/uaccess.h in current Linux),
# so mirror that instead of weakening the register check itself.
uaccess=arch/arm/include/asm/uaccess.h
grep -q 'register const typeof(\*(p)) __r2 asm("r2")' "$uaccess"
sed -i 's|register const typeof(\*(p)) __r2 asm("r2")|register typeof(*(p)) __r2 asm("r2")|' "$uaccess"
! grep -q 'register const typeof(\*(p)) __r2 asm("r2")' "$uaccess"

# Linux 2.6.33 predates modern GCC-specific compiler headers.  The ARM kernel
# still uses the GCC 4-compatible attribute definitions; expose that header
# under the detected major version so current reproducible runners can build it.
gcc_major=$(${CROSS}gcc -dumpfullversion -dumpversion | cut -d. -f1)
compiler_header="include/linux/compiler-gcc${gcc_major}.h"
if [ ! -e "$compiler_header" ]; then
  ln -s compiler-gcc4.h "$compiler_header"
fi

# Linux 2.6.33 has no reliable olddefconfig target.  Keep accepting defaults,
# but preserve make's exit code: `yes` normally receives SIGPIPE once Kconfig
# is done, which must not fail this pipefail-enabled build.
set +o pipefail
yes "" | make ARCH=arm CROSS_COMPILE="$CROSS" oldconfig
oldconfig_status=${PIPESTATUS[1]}
set -o pipefail
test "$oldconfig_status" -eq 0

# CONFIG_MODVERSIONS requires a complete matching build so Module.symvers is
# generated from the same source/config as the target kernel.
# LG built this tree with GCC 4.3, where plain `__inline` still followed GNU89
# semantics and emitted an externally visible out-of-line copy.  Current GCC
# defaults to C99 inline, which emits none -- so Nexell helpers such as
# NX_GPIO_SetBit end up as undefined references when vmlinux is linked.
# Restore the original semantics rather than editing every Nexell helper.
make -j2 ARCH=arm CROSS_COMPILE="$CROSS" KCFLAGS=-fgnu89-inline zImage modules

for module in usbnet cdc_ether rndis_host; do
  source="drivers/net/usb/$module.ko"
  test -s "$source"
  cp "$source" "$OUT/$module.ko"
  "${CROSS}strip" --strip-debug "$OUT/$module.ko"
done

{
  echo "kernel=2.6.33.7.2-rt30"
  echo "config=rk_hit_v2_ubif_defconfig"
  echo "source=https://github.com/larixer/kernel.rk"
  echo "source_commit=$(git -C .. rev-parse HEAD)"
  echo "compiler=$(${CROSS}gcc --version | head -n 1)"
  echo
  modinfo "$OUT/usbnet.ko" || true
  modinfo "$OUT/cdc_ether.ko" || true
  modinfo "$OUT/rndis_host.ko" || true
} > "$OUT/BUILDINFO.txt"

(cd "$OUT" && sha256sum *.ko > SHA256SUMS)
file "$OUT"/*.ko | tee "$OUT/FILEINFO.txt"
