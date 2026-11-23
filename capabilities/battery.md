# battery
This Mac's battery: how charged it is, and whether it runs on power or on
the battery. Read its snapshot (pmset's line: the percent, `charging`,
`discharging` or `charged`, and the time left); call `battery.status` to
read it again now.

probe: pmset -g batt
snapshot: pmset -g batt
interval: 60s

## tool: status
The battery now, as pmset reports it.
command: pmset -g batt
timeout: 10s

## event: unplugged
when: contains discharging
The Mac went on battery power (unplugged).

## event: plugged
when: contains AC Power
The Mac is on power again.
