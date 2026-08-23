#!/usr/bin/env bash
# Build kernel modules against LG's reconstructed 2.6.33.7.2-rt30 tree.
#
#   build-lg-kernel-modules.sh OUT_SUBDIR CONFIG_FRAGMENT MODULE_GLOB...
#
#   OUT_SUBDIR       directory under out/ to collect the result in
#   CONFIG_FRAGMENT  file with extra CONFIG_ lines, or "-" for none
#   MODULE_GLOB...   paths, relative to the kernel tree, of the .ko files to keep
#
# The four legacy-toolchain fixes live here and nowhere else, so every module
# built for this robot comes out of the same prepared tree.
#
# A note on CONFIG_FRAGMENT: the running kernel is fixed, and CONFIG_MODVERSIONS
# makes insmod compare a CRC per imported symbol. Adding options that change a
# built-in structure changes those CRCs, and the module is then correctly
# refused on the device. Keep fragments to switches that only add modules, and
# check the result with tools/verify-module-abi.py before going near the robot.

set -euo pipefail

OUT_SUBDIR=${1:?output subdirectory}
FRAGMENT=${2:?config fragment path or -}
shift 2
if [ "$#" -eq 0 ]; then
  echo "no module paths given" >&2
  exit 2
fi

ROOT="$(pwd)"
BUILD="$RUNNER_TEMP/hombot-kernel-build"
OUT="$ROOT/out/$OUT_SUBDIR"
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
# flag spelling used by 2.6.33.  2.6.33 spells these with and without spaces,
# and .piggydata carries only the alloc flag, so cover every form and then
# assert none survived.
grep -rlE ',[[:space:]]*#alloc' arch/arm | while IFS= read -r source; do
  sed -i -E \
    -e 's|,[[:space:]]*#alloc[[:space:]]*,[[:space:]]*#execinstr|, "ax", %progbits|g' \
    -e 's|,[[:space:]]*#alloc[[:space:]]*,[[:space:]]*#write|, "aw", %progbits|g' \
    -e 's|,[[:space:]]*#alloc[[:space:]]*$|, "a", %progbits|g' \
    "$source"
done
! grep -rq '#alloc' arch/arm

# Perl 5.22 removed `defined(@array)`, which kernel/timeconst.pl still uses to
# test whether a canned HZ table exists.  The bare array already has the right
# truthiness, so drop the defined() exactly as upstream did.
grep -q 'if (!defined(@val))' kernel/timeconst.pl
sed -i 's|if (!defined(@val))|if (!@val)|' kernel/timeconst.pl
perl -c kernel/timeconst.pl

# GCC 4.3 honoured `register const ... asm("r2")` even when the initialiser was
# a compile-time constant.  Current GCC folds such a variable into a constant
# and is free to keep it in any register, which trips the kernel's own
# __asmeq() register check -- e.g. `put_user(0, tsk->clear_child_tid)` in
# kernel/fork.c compiled to `.ifnc r3,r2 ; .err`.  Upstream dropped the `const`
# on this operand for exactly this reason, so mirror that instead of weakening
# the register check itself.
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

# Extra options, if any. Later lines win in a .config, but the "is not set"
# comments are removed first so the file stays readable and oldconfig has one
# unambiguous answer per symbol.
if [ "$FRAGMENT" != "-" ]; then
  test -s "$ROOT/$FRAGMENT"
  echo "applying config fragment:"
  cat "$ROOT/$FRAGMENT"
  while IFS= read -r line; do
    case "$line" in
      CONFIG_*=*)
        symbol=${line%%=*}
        sed -i "/^# ${symbol} is not set\$/d;/^${symbol}=/d" .config
        echo "$line" >> .config
        ;;
    esac
  done < "$ROOT/$FRAGMENT"
fi

# Linux 2.6.33 has no reliable olddefconfig target.  Keep accepting defaults,
# but preserve make's exit code: `yes` normally receives SIGPIPE once Kconfig
# is done, which must not fail this pipefail-enabled build.
set +o pipefail
yes "" | make ARCH=arm CROSS_COMPILE="$CROSS" oldconfig
oldconfig_status=${PIPESTATUS[1]}
set -o pipefail
test "$oldconfig_status" -eq 0

# oldconfig silently drops options whose dependencies are unmet, which would
# otherwise show up much later as a missing .ko. Fail here instead.
if [ "$FRAGMENT" != "-" ]; then
  while IFS= read -r line; do
    case "$line" in
      CONFIG_*=[ym])
        grep -qx "$line" .config || {
          echo "config fragment line did not survive oldconfig: $line" >&2
          exit 1
        }
        ;;
    esac
  done < "$ROOT/$FRAGMENT"
fi

# CONFIG_MODVERSIONS requires a complete matching build so Module.symvers is
# generated from the same source/config as the target kernel.
# LG built this tree with GCC 4.3, where plain `__inline` still followed GNU89
# semantics and emitted an externally visible out-of-line copy.  Current GCC
# defaults to C99 inline, which emits none -- so Nexell helpers such as
# NX_GPIO_SetBit end up as undefined references when vmlinux is linked.
# Restore the original semantics rather than editing every Nexell helper.
make -j2 ARCH=arm CROSS_COMPILE="$CROSS" KCFLAGS=-fgnu89-inline zImage modules

for pattern in "$@"; do
  found=0
  for source in $pattern; do
    test -s "$source" || continue
    name=$(basename "$source")
    cp "$source" "$OUT/$name"
    "${CROSS}strip" --strip-debug "$OUT/$name"
    found=1
  done
  if [ "$found" -eq 0 ]; then
    echo "no module matched: $pattern" >&2
    exit 1
  fi
done

{
  echo "kernel=2.6.33.7.2-rt30"
  echo "config=rk_hit_v2_ubif_defconfig"
  [ "$FRAGMENT" = "-" ] || echo "fragment=$FRAGMENT"
  echo "source=https://github.com/larixer/kernel.rk"
  echo "source_commit=$(git -C .. rev-parse HEAD)"
  echo "compiler=$(${CROSS}gcc --version | head -n 1)"
  echo
  for module in "$OUT"/*.ko; do
    modinfo "$module" || true
  done
} > "$OUT/BUILDINFO.txt"

(cd "$OUT" && sha256sum *.ko > SHA256SUMS)
file "$OUT"/*.ko | tee "$OUT/FILEINFO.txt"
