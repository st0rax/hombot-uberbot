# Hardware and expansion notes

## Known board

The observed mainboard is `EAX64466501-1.2 / EBR74308201`. Candidate unpopulated
headers include a six-pin row labelled CN14 and an eight-pin harness footprint
labelled CN8. Photos of the component side do not prove a pinout; traces pass
through vias and must be measured.

## UART

UART0 is the Linux/U-Boot console:

- `/dev/jiguart` links to `ttyS0`;
- 115200 baud, 8N1;
- 3.3-V logic is expected but must be measured before connection;
- U-Boot has a short boot delay and supports YMODEM loading.

CN14 is only a candidate for this console. Identify ground with power removed,
then measure levels and listen with a high-impedance logic analyzer before
connecting an adapter transmitter. Never connect RS-232 or 5-V TTL levels.

UART1 is the active Vision/Micom path at 230400 baud. Do not attach a transmitter
or parallel software reader while the original control stack is running.

## USB

The kernel exposes one EHCI root port. A passive Y cable does not create a second
data port. Simultaneous Wi-Fi and storage/debug hardware requires a genuine,
externally powered USB 2.0 hub designed not to back-feed the robot.

## I2C and peripherals

- I2C0 uses the NXP2120 GPIOB4/B5 pin functions.
- I2C1 uses GPIOB6/B7.
- The WM8960 audio codec is present on I2C.
- The POA030 camera is controlled over I2C and streams through the VIP/V4L2 path.

These are SoC/firmware mappings, not proof that signals are routed to an accessible
header. Trace and measure before adding hardware.

## Power

Keep power-system experiments outside the software control layer. Do not suppress
real low-voltage protection or emulate a healthy battery state. Use a current-
limited, correctly polarized supply and independent measurements when developing
on the bench.
