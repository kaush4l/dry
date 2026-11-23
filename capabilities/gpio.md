# gpio
A Raspberry Pi's general-purpose pins (Pi OS, the `pinctrl` tool): read a
pin's level and mode, or set one as an output, high or low. Pins are
numbered as BCM GPIO numbers (GPIO17 is pin 17). Setting a pin drives real
hardware, so it needs approval.

probe: test -e /dev/gpiochip0
snapshot: pinctrl get 2-27
interval: 10s

## tool: read
One pin's mode and level, as pinctrl reports it.
command: pinctrl get {pin}
inputs:
- pin (integer, required): the BCM GPIO number, 0 to 27

## tool: set
Make a pin an output and drive it: level `dh` (high) or `dl` (low).
approval: yes
command: pinctrl set {pin} op {level}
timeout: 5s
inputs:
- pin (integer, required): the BCM GPIO number, 0 to 27
- level (string, required): dh (high) or dl (low)

## event: button
when: contains | lo // GPIO17 
GPIO17 went low (a button on GPIO17 was pressed).
