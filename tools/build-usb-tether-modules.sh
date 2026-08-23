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

# New ARM binutils treat `#` as a comment marker and reject the legacy section
# flag spelling used by 2.6.33.  Convert it to the equivalent ELF spelling.
grep -rl ', #alloc, #execinstr' arch/arm | while IFS= read -r source; do
  sed -i 's|, #alloc, #execinstr|, "ax", %progbits|g' "$source"
done

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
make -j2 ARCH=arm CROSS_COMPILE="$CROSS" zImage modules

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
