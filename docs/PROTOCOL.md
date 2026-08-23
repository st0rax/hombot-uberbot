# LG SmartControl protocol notes

These notes describe an interoperability interface reconstructed from the local
LG service and observed traffic. Treat every undocumented field as provisional.

## Connection sequence

1. Discover the robot's active interface address, or use the configured override,
   and connect to the device-local session service on TCP 4002.
2. Send `{"CONNECT":"REQUEST"}` in an application frame.
3. Wait for `{"CONNECT":"ENABLE"}`.
4. Connect to the device-local command/status service on TCP 4000 using the same
   address.
5. Send `{"SESSION":"ALIVE"}` periodically and read status messages.

TCP 9000 is an internal broker listener and is not used by the daemon. Earlier
probing left connections in `CLOSE_WAIT`, so experiments should remain on the
known 4002/4000 path.

Although historic source uses the loopback address, the observed 2.6.33 device
does not route its nominal loopback range correctly. The LG listeners are available
on the active interface, which is why `hombotd` resolves that address dynamically.

## Frame format

The application frame starts with 12 bytes, followed by the JSON payload:

| Offset | Size | Meaning |
|---:|---:|---|
| 0 | 1 | magic `0x0d` |
| 1 | 1 | payload type `0x04` for JSON |
| 2 | 1 | packet identifier |
| 3 | 1 | flags/reserved |
| 4 | 2 | part index, little-endian |
| 6 | 2 | part count, little-endian |
| 8 | 2 | payload length, little-endian |
| 10 | 2 | reserved |
| 12 | variable | JSON payload |

The parser must handle partial reads, multiple frames in one read and invalid
lengths. Cap payload length before allocation. Packet fragmentation fields are
known but multipart reassembly should not be claimed until tested.

## Read-only status fields

Observed messages can contain `ROBOT_STATE`, `TURBO`, `REPEAT`, `BATT`, `MODE`,
`NICKNAME`, `VERSION` and `SESSION`. Historic clients map the small integer
`BATT` level to 20-percent steps. Negative values are sentinels, not a voltage.

Do not send the diagnosis request used by historic clients during session setup.
The built-in diagnostic state machine can move the robot.

## Raw sensors

Static analysis identifies message types for a normal sensor record (84 bytes),
accelerometer/bumper record (40 bytes), raw sensor record (158 bytes) and an
extended diagnostic record (28 bytes). The correct live integration point is a
read-only broker subscriber, not a second process reading the Micom UART.

A factory decoder has a passive raw-sensor subscription branch. Other adjacent
factory commands can erase blackbox data or drive motors, so any adapter must use
a strict allowlist and have no generic send primitive.

One provisional battery hypothesis is a little-endian 16-bit value at raw-sensor
offset `+5`, scaled as hundredths of a volt. This must remain marked unconfirmed
until correlated against several simultaneous multimeter readings.
